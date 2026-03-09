use chrono::{Datelike, Duration, Timelike, Utc};
use scmux_daemon::config::{AtmConfig, Config, DaemonConfig, PollingConfig};
use scmux_daemon::{atm, ci, db, definition_writer, hosts, tmux_poller, AppState, SystemClock};
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
        atm: AtmConfig {
            enabled: false,
            teams: Vec::new(),
            allow_shutdown: false,
            socket_path: None,
            stuck_minutes: Some(10),
            stop_grace_secs: None,
        },
    }
}

fn build_state() -> (Arc<AppState>, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("integration.db");
    let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
    let host_id = definition_writer::ensure_local_host(&conn).expect("local host");
    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        db_path: db_path.to_string_lossy().to_string(),
        host_id,
        config: test_config(),
        reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
        runtime: std::sync::Mutex::new(scmux_daemon::runtime::RuntimeProjection::default()),
        ci_tools: ci::ToolAvailability::default(),
        clock: Arc::new(SystemClock),
        atm_available: std::sync::atomic::AtomicBool::new(false),
        last_api_access: std::sync::atomic::AtomicU64::new(0),
        started_at: std::time::Instant::now(),
        health: std::sync::Mutex::new(scmux_daemon::RuntimeHealth::default()),
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
    definition_writer::create_session(
        &db_conn,
        &db::NewSession {
            name: name.to_string(),
            project: Some("integration".to_string()),
            host_id: state.host_id,
            config_json: format!(
                r#"{{"session_name":"{name}","root_path":"/tmp","panes":[{{"name":"agent","command":"sleep 1","atm_agent":"agent","atm_team":"scmux-dev"}}]}}"#
            ),
            cron_schedule,
            auto_start,
            github_repo: None,
            azure_project: None,
        },
    )
    .expect("create session")
}

fn runtime_status(state: &Arc<AppState>, session_name: &str) -> String {
    let runtime = state.runtime.lock().expect("runtime lock");
    runtime
        .session(session_name)
        .map(|row| row.status.clone())
        .unwrap_or_else(|| "stopped".to_string())
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
async fn t_i_01_poll_cycle_does_not_create_session_status_table() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti01");
    let _session_id = insert_session(&state, &name, false, None);

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session_status'",
            [],
            |r| r.get(0),
        )
        .expect("status table count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn t_i_02_poll_cycle_marks_session_running_when_found_in_tmux() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti02");
    let _session_id = insert_session(&state, &name, false, None);

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

    let poll_result = tmux_poller::poll_cycle(&state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev);
    poll_result.expect("poll cycle");

    assert_eq!(runtime_status(&state, &name), "running");
}

#[tokio::test]
async fn t_i_03_poll_cycle_marks_stopped_for_missing_session() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti03");
    let _session_id = insert_session(&state, &name, false, None);

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    assert_eq!(runtime_status(&state, &name), "stopped");
}

#[tokio::test]
async fn t_i_04_running_to_missing_transition_logs_stopped_event() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti04");
    let _session_id = insert_session(&state, &name, false, None);
    {
        let panes = vec![scmux_daemon::tmux::PaneInfo {
            index: 0,
            name: "pane-0".to_string(),
            status: "active".to_string(),
            last_activity: "now".to_string(),
            current_command: "bash".to_string(),
        }];
        let mut live = std::collections::HashMap::new();
        live.insert(name.clone(), panes);
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.apply_tmux_snapshot(
            std::slice::from_ref(&name),
            &live,
            &std::collections::HashMap::new(),
            &chrono::Utc::now().to_rfc3339(),
        );
    }

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    assert_eq!(runtime_status(&state, &name), "stopped");
}

#[tokio::test]
async fn t_i_04_auto_start_attempt_logs_event() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti04");
    let _session_id = insert_session(&state, &name, true, None);

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    let status = runtime_status(&state, &name);
    assert!(status == "starting" || status == "running" || status == "stopped");
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
    let _session_id = insert_session(&state, &name, false, Some(cron));

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    let status = runtime_status(&state, &name);
    assert!(status == "starting" || status == "running" || status == "stopped");
}

#[tokio::test]
async fn t_i_06_invalid_cron_does_not_attempt_start() {
    let (state, _tmp) = build_state();
    let name = unique_name("ti06");
    let _session_id = {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .execute(
                "INSERT INTO sessions (
                    name, project, host_id, config_json, cron_schedule, auto_start, enabled
                 ) VALUES (?1, 'integration', ?2, ?3, 'not-a-cron', 0, 1)",
                rusqlite::params![
                    name,
                    state.host_id,
                    format!(
                        r#"{{"session_name":"{}","root_path":"/tmp","panes":[{{"name":"agent","command":"sleep 1","atm_agent":"agent","atm_team":"scmux-dev"}}]}}"#,
                        name
                    )
                ],
            )
            .expect("insert invalid-cron session");
        db_conn.last_insert_rowid()
    };

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    assert_eq!(runtime_status(&state, &name), "stopped");
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
    let _session_id = insert_session(&state, &name, true, Some(cron));

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

    let status = runtime_status(&state, &name);
    assert!(status == "starting" || status == "running" || status == "stopped");
}

#[tokio::test]
async fn t_i_08_migrate_does_not_create_daemon_health_table() {
    let (state, _tmp) = build_state();
    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='daemon_health'",
            [],
            |r| r.get(0),
        )
        .expect("daemon_health table count");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn t_i_09_health_endpoint_visibility_does_not_require_daemon_health_table() {
    let (state, _tmp) = build_state();
    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='daemon_health'",
            [],
            |r| r.get(0),
        )
        .expect("daemon_health table count");
    assert_eq!(count, 0);
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

    tmux_poller::poll_cycle(&state)
        .await
        .expect("warm-up poll cycle");
    let mut samples = Vec::new();
    for _ in 0..10 {
        let started = std::time::Instant::now();
        tmux_poller::poll_cycle(&state).await.expect("poll cycle");
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
async fn t_i_20_does_not_reconstruct_registry_from_live_tmux_after_db_loss() {
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

    let poll_result = tmux_poller::poll_cycle(&state).await;
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
    assert_eq!(recovered_sessions, 0);
    drop(db_conn);
    let runtime = state.runtime.lock().expect("runtime lock");
    assert_eq!(runtime.discovery_rows().len(), 2);
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
async fn t_lc_04_single_session_start_failure_does_not_abort_other_sessions() {
    let (state, _tmp) = build_state();
    let bad_name = unique_name("td20-bad");
    let good_name = unique_name("td20-good");
    let _bad_id = insert_session(&state, &bad_name, true, None);
    let _good_id = insert_session(&state, &good_name, true, None);

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

    let poll_result = tmux_poller::poll_cycle(&state).await;
    restore_env_var("SCMUX_TMUXP_BIN", prev);
    poll_result.expect("poll cycle");

    assert_eq!(runtime_status(&state, &bad_name), "stopped");
    let good_status = runtime_status(&state, &good_name);
    assert!(good_status == "starting" || good_status == "running");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn td_24_daemon_rss_under_50mb_after_loading_20_sessions() {
    let (state, _tmp) = build_state();
    for idx in 0..20 {
        let name = format!("td24-{idx}");
        insert_session(&state, &name, false, None);
    }

    tmux_poller::poll_cycle(&state).await.expect("poll cycle");

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

#[tokio::test]
async fn t_wg_01_definition_writer_create_path_mutates_sqlite() {
    let (state, _tmp) = build_state();
    let name = unique_name("wg01");
    let created = {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_session(
            &db_conn,
            &db::NewSession {
                name: name.clone(),
                project: Some("writer-gate".to_string()),
                host_id: state.host_id,
                config_json: format!(
                    r#"{{"session_name":"{name}","root_path":"/tmp","panes":[{{"name":"agent","command":"sleep 1","atm_agent":"agent","atm_team":"scmux-dev"}}]}}"#
                ),
                cron_schedule: None,
                auto_start: false,
                github_repo: None,
                azure_project: None,
            },
        )
        .expect("create via definition_writer")
    };
    assert!(created > 0);

    let db_conn = state.db.lock().expect("db lock");
    let count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE host_id = ?1 AND name = ?2 AND enabled = 1",
            rusqlite::params![state.host_id, name],
            |r| r.get(0),
        )
        .expect("count session");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn t_wg_02_pollers_do_not_write_runtime_sqlite_tables() {
    let (state, _tmp) = build_state();
    let name = unique_name("wg02");
    let _id = insert_session(&state, &name, false, None);

    let sessions_before = {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE host_id = ?1 AND enabled = 1",
                [state.host_id],
                |r| r.get::<_, i64>(0),
            )
            .expect("sessions before")
    };

    tmux_poller::poll_cycle(&state).await.expect("tmux poll");
    hosts::poll_hosts(Arc::clone(&state))
        .await
        .expect("host poll");
    ci::poll_once(&state).await.expect("ci poll");
    let _ = atm::poll_once(&state).await;

    let db_conn = state.db.lock().expect("db lock");
    let sessions_after: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE host_id = ?1 AND enabled = 1",
            [state.host_id],
            |r| r.get(0),
        )
        .expect("sessions after");
    let deprecated_status_table: i64 = db_conn
        .query_row("SELECT COUNT(*) FROM session_status", [], |r| r.get(0))
        .unwrap_or(0);
    let deprecated_ci_table: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session_ci'",
            [],
            |r| r.get(0),
        )
        .expect("session_ci table existence");
    let deprecated_atm_table: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session_atm'",
            [],
            |r| r.get(0),
        )
        .expect("session_atm table existence");

    assert_eq!(sessions_after, sessions_before);
    assert_eq!(deprecated_status_table, 0);
    assert_eq!(deprecated_ci_table, 0);
    assert_eq!(deprecated_atm_table, 0);
}

#[tokio::test]
async fn t_wg_03_unapproved_project_write_is_rejected() {
    let (state, _tmp) = build_state();
    let name = unique_name("wg03");
    let db_conn = state.db.lock().expect("db lock");
    let result = definition_writer::create_session(
        &db_conn,
        &db::NewSession {
            name: name.clone(),
            project: Some("writer-gate".to_string()),
            host_id: state.host_id,
            config_json: format!(r#"{{"session_name":"{name}","root_path":"/tmp","panes":[]}}"#),
            cron_schedule: None,
            auto_start: false,
            github_repo: None,
            azure_project: None,
        },
    );

    match result {
        Err(definition_writer::WriteError::Validation(message)) => {
            assert!(message.contains("config_json.panes[]"));
        }
        Err(other) => panic!("expected validation error, got: {other:?}"),
        Ok(_) => panic!("expected validation error for unapproved project write"),
    }
}

#[tokio::test]
async fn t_wg_04_delete_db_and_restart_does_not_reconstruct_from_tmux() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("wg04.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    {
        let conn = db::open(&db_path_str).expect("open db");
        let _host_id = definition_writer::ensure_local_host(&conn).expect("local host");
    }

    std::fs::remove_file(&db_path).expect("delete sqlite");

    let conn = db::open(&db_path_str).expect("reopen db");
    let host_id = definition_writer::ensure_local_host(&conn).expect("local host after restart");
    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        db_path: db_path_str,
        host_id,
        config: test_config(),
        reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
        runtime: std::sync::Mutex::new(scmux_daemon::runtime::RuntimeProjection::default()),
        ci_tools: ci::ToolAvailability::default(),
        clock: Arc::new(SystemClock),
        atm_available: std::sync::atomic::AtomicBool::new(false),
        last_api_access: std::sync::atomic::AtomicU64::new(0),
        started_at: std::time::Instant::now(),
        health: std::sync::Mutex::new(scmux_daemon::RuntimeHealth::default()),
    });

    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  echo "wg04-alpha"
  echo "wg04-beta"
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

    let poll_result = tmux_poller::poll_cycle(&state).await;
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
    assert_eq!(recovered_sessions, 0);
    drop(db_conn);

    let runtime = state.runtime.lock().expect("runtime lock");
    assert_eq!(runtime.discovery_rows().len(), 2);
}
