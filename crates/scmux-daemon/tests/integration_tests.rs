use chrono::{Datelike, Duration, Timelike, Utc};
use scmux_daemon::config::{Config, DaemonConfig, PollingConfig};
use scmux_daemon::{db, scheduler, AppState};
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
        host_id,
        config: test_config(),
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
