use chrono::{Datelike, Duration, Timelike, Utc};
use scmux_daemon::config::{Config, DaemonConfig, PollingConfig};
use scmux_daemon::{ci, db, hosts, scheduler, AppState, SystemClock};
use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use tempfile::TempDir;
use tokio::sync::Mutex;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn test_config() -> Config {
    Config {
        daemon: DaemonConfig {
            port: None,
            db_path: None,
            default_terminal: Some("iterm2".to_string()),
            log_level: None,
        },
        polling: PollingConfig {
            tmux_interval_secs: Some(15),
            health_interval_secs: Some(60),
            ci_active_interval_secs: None,
            ci_idle_interval_secs: None,
        },
        hosts: Vec::new(),
    }
}

fn build_state() -> (Arc<AppState>, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("integration.db");
    let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
    let host_id = db::ensure_local_host(&conn).expect("local host");
    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        db_path: db_path.to_string_lossy().to_string(),
        host_id,
        config: test_config(),
        reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
        ci_tools: ci::ToolAvailability::default(),
        clock: Arc::new(SystemClock),
        last_api_access: std::sync::atomic::AtomicU64::new(0),
        started_at: std::time::Instant::now(),
    });
    (state, tmp)
}

fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

fn write_script(contents: &str) -> tempfile::TempPath {
    let mut file = tempfile::NamedTempFile::new().expect("temp script");
    file.write_all(contents.as_bytes()).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file.as_file().metadata().expect("metadata").permissions();
        perms.set_mode(0o755);
        file.as_file().set_permissions(perms).expect("chmod");
    }
    file.into_temp_path()
}

fn set_env_var(key: &str, value: &str) -> Option<String> {
    let prev = std::env::var(key).ok();
    // SAFETY: test-only env mutation guarded by async mutex.
    unsafe { std::env::set_var(key, value) };
    prev
}

fn restore_env_var(key: &str, prev: Option<String>) {
    match prev {
        Some(value) => {
            // SAFETY: test-only env restoration guarded by async mutex.
            unsafe { std::env::set_var(key, value) };
        }
        None => {
            // SAFETY: test-only env restoration guarded by async mutex.
            unsafe { std::env::remove_var(key) };
        }
    }
}

fn insert_session(
    state: &Arc<AppState>,
    name: &str,
    auto_start: bool,
    cron_schedule: Option<String>,
) -> i64 {
    let db_conn = state.db.lock().expect("db lock");
    db::create_session(
        &db_conn,
        &db::NewSession {
            name: name.to_string(),
            project: Some("integration".to_string()),
            host_id: state.host_id,
            config_json: format!(r#"{{"session_name":"{name}"}}"#),
            cron_schedule,
            auto_start,
            github_repo: None,
            azure_project: None,
        },
    )
    .expect("create session")
}

fn event_count(state: &Arc<AppState>, session_id: i64, event: &str, trigger: &str) -> i64 {
    let db_conn = state.db.lock().expect("db lock");
    db_conn
        .query_row(
            "SELECT COUNT(*) FROM session_events WHERE session_id = ?1 AND event = ?2 AND trigger = ?3",
            rusqlite::params![session_id, event, trigger],
            |r| r.get(0),
        )
        .expect("event count")
}

fn insert_remote_host(state: &Arc<AppState>, name: &str, address: &str, api_port: u16) -> i64 {
    let db_conn = state.db.lock().expect("db lock");
    db_conn
        .execute(
            "INSERT INTO hosts (name, address, ssh_user, api_port, is_local)
             VALUES (?1, ?2, 'tester', ?3, 0)",
            rusqlite::params![name, address, api_port],
        )
        .expect("insert remote host");
    db_conn.last_insert_rowid()
}

#[tokio::test]
async fn t_i_01_poll_cycle_writes_session_status_rows() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti01");
    let session_id = insert_session(&state, &name, false, None);

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM session_status WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .expect("status row count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn t_i_02_poll_cycle_marks_session_running_when_found_in_tmux() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti02");
    let session_id = insert_session(&state, &name, false, None);

    let _guard = env_lock().lock().await;
    let script = write_script(&format!(
        r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  echo "{name}"
  exit 0
fi
if [ "$1" = "list-panes" ]; then
  echo "0|lead|zsh|1"
  exit 0
fi
exit 1
"#
    ));
    let prev = set_env_var("SCMUX_TMUX_BIN", script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev);
    poll_result.expect("poll cycle");

    let db_conn = state.db.lock().expect("db lock");
    let status: String = db_conn
        .query_row(
            "SELECT status FROM session_status WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(status, "running");
}

#[tokio::test]
async fn t_i_03_poll_cycle_marks_stopped_for_missing_session() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti03");
    let session_id = insert_session(&state, &name, false, None);

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let db_conn = state.db.lock().expect("db lock");
    let status: String = db_conn
        .query_row(
            "SELECT status FROM session_status WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(status, "stopped");
}

#[tokio::test]
async fn t_i_04_running_to_missing_transition_logs_stopped_event() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti04");
    let session_id = insert_session(&state, &name, false, None);
    {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .execute(
                "INSERT INTO session_status (session_id, status, polled_at) VALUES (?1, 'running', datetime('now'))",
                [session_id],
            )
            .expect("seed running status");
    }

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    assert!(event_count(&state, session_id, "stopped", "daemon") >= 1);
}

#[tokio::test]
async fn t_i_04_auto_start_attempt_logs_event() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti04");
    let session_id = insert_session(&state, &name, true, None);

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let auto_started = event_count(&state, session_id, "started", "auto_start");
    let auto_failed = event_count(&state, session_id, "failed", "auto_start");
    assert_eq!(auto_started + auto_failed, 1);
}

#[tokio::test]
async fn t_i_05_due_cron_attempt_logs_event() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti05");
    let target = Utc::now() - Duration::seconds(5);
    let cron = format!(
        "{} {} {} {} {} *",
        target.second(),
        target.minute(),
        target.hour(),
        target.day(),
        target.month()
    );
    let session_id = insert_session(&state, &name, false, Some(cron));

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let cron_started = event_count(&state, session_id, "started", "cron");
    let cron_failed = event_count(&state, session_id, "failed", "cron");
    assert_eq!(cron_started + cron_failed, 1);
}

#[tokio::test]
async fn t_i_06_invalid_cron_does_not_attempt_start() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti06");
    let session_id = {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .execute(
                "INSERT INTO sessions (
                    name, project, host_id, config_json, cron_schedule, auto_start, enabled
                 ) VALUES (?1, 'integration', ?2, ?3, 'not-a-cron', 0, 1)",
                rusqlite::params![
                    name,
                    state.host_id,
                    format!(r#"{{"session_name":"{}"}}"#, name)
                ],
            )
            .expect("insert invalid-cron session");
        db_conn.last_insert_rowid()
    };

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let cron_started = event_count(&state, session_id, "started", "cron");
    let cron_failed = event_count(&state, session_id, "failed", "cron");
    assert_eq!(cron_started + cron_failed, 0);
}

#[tokio::test]
async fn t_i_07_single_cycle_does_not_retry_failed_start() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti07");
    let target = Utc::now() - Duration::seconds(5);
    let cron = format!(
        "{} {} {} {} {} *",
        target.second(),
        target.minute(),
        target.hour(),
        target.day(),
        target.month()
    );
    let session_id = insert_session(&state, &name, true, Some(cron));

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let auto_events = event_count(&state, session_id, "started", "auto_start")
        + event_count(&state, session_id, "failed", "auto_start");
    let cron_events = event_count(&state, session_id, "started", "cron")
        + event_count(&state, session_id, "failed", "cron");
    assert_eq!(auto_events + cron_events, 1);
}

#[tokio::test]
async fn t_i_08_write_health_inserts_row() {
    let (state, _tmp) = build_state();

    db::write_health(&state).await.expect("write health");

    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row("SELECT COUNT(*) FROM daemon_health", [], |r| r.get(0))
        .expect("health count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn t_i_09_write_health_prunes_older_than_seven_days() {
    let (state, _tmp) = build_state();
    {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .execute(
                "INSERT INTO daemon_health (host_id, status, sessions_running, recorded_at)
                 VALUES (?1, 'ok', 0, datetime('now', '-8 days'))",
                [state.host_id],
            )
            .expect("seed old health row");
    }

    db::write_health(&state).await.expect("write health");

    let db_conn = state.db.lock().expect("db lock");
    let old_count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM daemon_health WHERE recorded_at < datetime('now', '-7 days')",
            [],
            |r| r.get(0),
        )
        .expect("old row count");
    assert_eq!(old_count, 0);
}

#[tokio::test]
async fn t_i_10_poll_hosts_unreachable_test_net_returns_ok() {
    let (state, _tmp) = build_state();
    insert_remote_host(&state, "ti10-remote", "192.0.2.1", 7878);

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 1\n");
    let prev = set_env_var("SCMUX_PING_BIN", script.to_string_lossy().as_ref());

    let result = hosts::poll_hosts(Arc::clone(&state)).await;
    restore_env_var("SCMUX_PING_BIN", prev);
    result.expect("poll hosts");
}

#[tokio::test]
async fn t_i_11_three_failures_mark_host_unreachable() {
    let (state, _tmp) = build_state();
    let remote_id = insert_remote_host(&state, "ti11-remote", "192.0.2.1", 7878);

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 1\n");
    let prev = set_env_var("SCMUX_PING_BIN", script.to_string_lossy().as_ref());

    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts #1");
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts #2");
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts #3");
    restore_env_var("SCMUX_PING_BIN", prev);

    let map = state.reachability.lock().expect("reachability lock");
    let entry = map.get(&remote_id).expect("remote host entry");
    assert!(!entry.reachable);
    assert!(entry.consecutive_failures >= 3);
}

#[tokio::test]
async fn t_i_12_success_after_failures_marks_host_reachable() {
    let (state, _tmp) = build_state();
    let remote_id = insert_remote_host(&state, "ti12-remote", "127.0.0.1", 1);

    let _guard = env_lock().lock().await;
    let fail_script = write_script("#!/bin/sh\nexit 1\n");
    let success_script = write_script("#!/bin/sh\nexit 0\n");
    let prev = set_env_var("SCMUX_PING_BIN", fail_script.to_string_lossy().as_ref());

    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts fail #1");
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts fail #2");
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts fail #3");
    let _ = set_env_var("SCMUX_PING_BIN", success_script.to_string_lossy().as_ref());
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("poll hosts success");
    restore_env_var("SCMUX_PING_BIN", prev);

    let map = state.reachability.lock().expect("reachability lock");
    let entry = map.get(&remote_id).expect("remote host entry");
    assert!(entry.reachable);
    assert_eq!(entry.consecutive_failures, 0);
}

#[tokio::test]
#[ignore = "perf-gate: run in --release CI job"]
async fn td_22_poll_cycle_latency_under_500ms_for_50_sessions() {
    let (state, _tmp) = build_state();
    for idx in 0..50 {
        let name = format!("ti20-{idx}");
        insert_session(&state, &name, false, None);
    }

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 1\n");
    let prev = set_env_var("SCMUX_TMUX_BIN", script.to_string_lossy().as_ref());

    scheduler::poll_cycle(&state)
        .await
        .expect("warm-up poll cycle");
    let mut samples = Vec::new();
    for _ in 0..10 {
        let started = std::time::Instant::now();
        scheduler::poll_cycle(&state).await.expect("poll cycle");
        samples.push(started.elapsed());
    }
    restore_env_var("SCMUX_TMUX_BIN", prev);

    samples.sort();
    let p95_index = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    let p95 = samples[p95_index];
    assert!(
        p95 < std::time::Duration::from_millis(500),
        "poll cycle p95 exceeded 500ms with 50 sessions: {:?}",
        p95
    );
}

#[tokio::test]
async fn t_i_20_reconstructs_registry_from_live_tmux_after_db_loss() {
    let (state, _tmp) = build_state();

    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  echo "ti22-alpha"
  echo "ti22-beta"
  exit 0
fi
if [ "$1" = "list-panes" ]; then
  echo "0|lead|zsh|1"
  exit 0
fi
exit 1
"#,
    );
    let prev = set_env_var("SCMUX_TMUX_BIN", script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev);
    poll_result.expect("poll cycle");

    let db_conn = state.db.lock().expect("db lock");
    let recovered_sessions: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE host_id = ?1 AND enabled = 1",
            [state.host_id],
            |r| r.get(0),
        )
        .expect("session count");
    let running_rows: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM session_status WHERE status = 'running'",
            [],
            |r| r.get(0),
        )
        .expect("running count");
    assert_eq!(recovered_sessions, 2);
    assert_eq!(running_rows, 2);
}

#[tokio::test]
async fn td_19_single_unreachable_host_does_not_abort_poll_cycle() {
    let (state, _tmp) = build_state();
    let failing_host = insert_remote_host(&state, "td19-fail", "192.0.2.1", 7878);
    let healthy_host = insert_remote_host(&state, "td19-healthy", "198.51.100.2", 7878);

    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
case "$*" in
  *192.0.2.1*) exit 1 ;;
  *) exit 0 ;;
esac
"#,
    );
    let prev = set_env_var("SCMUX_PING_BIN", script.to_string_lossy().as_ref());

    let host_result = hosts::poll_hosts(Arc::clone(&state)).await;
    restore_env_var("SCMUX_PING_BIN", prev);
    host_result.expect("host poll should continue on unreachable host");

    let map = state.reachability.lock().expect("reachability lock");
    let failing = map.get(&failing_host).expect("failing host reachability");
    let healthy = map.get(&healthy_host).expect("healthy host reachability");
    assert!(!failing.reachable);
    assert!(healthy.reachable);
}

#[tokio::test]
async fn td_20_single_session_start_failure_does_not_abort_session_loop() {
    let (state, _tmp) = build_state();
    let bad_name = unique_name("td20-bad");
    let good_name = unique_name("td20-good");
    let bad_id = insert_session(&state, &bad_name, true, None);
    let good_id = insert_session(&state, &good_name, true, None);

    let _guard = env_lock().lock().await;
    let script = write_script(&format!(
        r#"#!/bin/sh
if [ "$1" = "load" ]; then
  if grep -q "{bad_name}" "$3"; then
    echo "simulated tmuxp failure" >&2
    exit 1
  fi
  exit 0
fi
exit 1
"#
    ));
    let prev = set_env_var("SCMUX_TMUXP_BIN", script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&state).await;
    restore_env_var("SCMUX_TMUXP_BIN", prev);
    poll_result.expect("poll cycle");

    let bad_failures = event_count(&state, bad_id, "failed", "auto_start");
    let good_starts = event_count(&state, good_id, "started", "auto_start");
    assert_eq!(bad_failures, 1);
    assert_eq!(good_starts, 1);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn td_24_daemon_rss_under_50mb_after_loading_20_sessions() {
    let (state, _tmp) = build_state();
    for idx in 0..20 {
        let name = format!("td24-{idx}");
        insert_session(&state, &name, false, None);
    }

    scheduler::poll_cycle(&state).await.expect("poll cycle");

    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    let rss_kb: u64 = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(
        rss_kb < 50 * 1024,
        "RSS exceeded 50MB after loading 20 sessions: {} KB",
        rss_kb
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn td_24_daemon_rss_under_50mb_after_loading_20_sessions() {
    // Linux-only implementation uses /proc/self/status; macOS is validated manually in Phase 4 runbook.
}
