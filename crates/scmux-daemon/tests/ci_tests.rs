use chrono::DateTime;
use scmux_daemon::ci::{self, ToolAvailability};
use scmux_daemon::config::{AtmConfig, Config, DaemonConfig, PollingConfig};
use scmux_daemon::{db, definition_writer, tmux::PaneInfo, AppState, SystemClock};
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
            socket_path: None,
            stuck_minutes: Some(10),
            stop_grace_secs: None,
        },
        hosts: Vec::new(),
    }
}

fn build_state(ci_tools: ToolAvailability) -> (Arc<AppState>, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("ci-tests.db");
    let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
    let host_id = definition_writer::ensure_local_host(&conn).expect("local host");
    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        db_path: db_path.to_string_lossy().to_string(),
        host_id,
        config: test_config(),
        reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
        runtime: std::sync::Mutex::new(scmux_daemon::runtime::RuntimeProjection::default()),
        ci_tools,
        clock: Arc::new(SystemClock),
        atm_available: std::sync::atomic::AtomicBool::new(false),
        last_api_access: std::sync::atomic::AtomicU64::new(0),
        started_at: std::time::Instant::now(),
    });
    (state, tmp)
}

fn insert_ci_session(
    state: &Arc<AppState>,
    name: &str,
    github_repo: Option<&str>,
    azure_project: Option<&str>,
    panes_json: &str,
) -> i64 {
    let db_conn = state.db.lock().expect("db lock");
    let session_id = definition_writer::create_session(
        &db_conn,
        &db::NewSession {
            name: name.to_string(),
            project: Some("ci".to_string()),
            host_id: state.host_id,
            config_json: format!(
                r#"{{"session_name":"{name}","panes":[{{"name":"agent","command":"sleep 1","atm_agent":"agent","atm_team":"scmux-dev"}}]}}"#
            ),
            cron_schedule: None,
            auto_start: false,
            github_repo: github_repo.map(ToString::to_string),
            azure_project: azure_project.map(ToString::to_string),
        },
    )
    .expect("create session");
    drop(db_conn);

    let panes = serde_json::from_str::<serde_json::Value>(panes_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(idx, pane)| PaneInfo {
            index: pane
                .get("index")
                .and_then(|raw| raw.as_u64())
                .unwrap_or(idx as u64) as u32,
            name: pane
                .get("name")
                .and_then(|raw| raw.as_str())
                .unwrap_or("pane")
                .to_string(),
            status: pane
                .get("status")
                .and_then(|raw| raw.as_str())
                .unwrap_or("idle")
                .to_string(),
            last_activity: pane
                .get("last_activity")
                .and_then(|raw| raw.as_str())
                .unwrap_or("unknown")
                .to_string(),
            current_command: pane
                .get("current_command")
                .and_then(|raw| raw.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect::<Vec<_>>();
    let mut live = std::collections::HashMap::new();
    live.insert(name.to_string(), panes);
    let mut runtime = state.runtime.lock().expect("runtime lock");
    runtime.apply_tmux_snapshot(
        &[name.to_string()],
        &live,
        &std::collections::HashMap::new(),
        &chrono::Utc::now().to_rfc3339(),
    );

    session_id
}

fn ci_row(
    state: &Arc<AppState>,
    session_id: i64,
    provider: &str,
) -> (String, Option<String>, String, String) {
    let session_name = {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT name FROM sessions WHERE id = ?1",
                rusqlite::params![session_id],
                |r| r.get::<_, String>(0),
            )
            .expect("session name")
    };

    let runtime = state.runtime.lock().expect("runtime lock");
    let row = runtime
        .ci_for_session(&session_name)
        .into_iter()
        .find(|entry| entry.provider == provider)
        .expect("ci row");
    (
        row.status,
        row.tool_message,
        row.polled_at.expect("polled_at"),
        row.next_poll_at.expect("next_poll_at"),
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

#[test]
fn td_10_ci_interval_is_1_minute_when_any_pane_is_active() {
    assert_eq!(ci::next_interval(true).as_secs(), 60);
}

#[test]
fn td_11_ci_interval_is_5_minutes_when_all_panes_are_idle() {
    assert_eq!(ci::next_interval(false).as_secs(), 300);
}

#[tokio::test]
async fn td_12_tool_unavailable_recorded_when_gh_missing() {
    let (state, _tmp) = build_state(ToolAvailability {
        gh_available: false,
        az_available: true,
    });
    let session_id = insert_ci_session(
        &state,
        "td12-gh-missing",
        Some("acme/repo"),
        None,
        r#"[{"name":"agent","status":"active"}]"#,
    );

    ci::poll_once(&state).await.expect("poll once");

    let (status, tool_message, polled_at, next_poll_at) = ci_row(&state, session_id, "github");
    assert_eq!(status, "tool_unavailable");
    assert!(tool_message
        .as_deref()
        .is_some_and(|msg| msg.contains("brew install gh")));
    assert!(DateTime::parse_from_rfc3339(&polled_at).is_ok());
    assert!(DateTime::parse_from_rfc3339(&next_poll_at).is_ok());
}

#[tokio::test]
async fn td_13_tool_unavailable_recorded_when_az_missing() {
    let (state, _tmp) = build_state(ToolAvailability {
        gh_available: true,
        az_available: false,
    });
    let session_id = insert_ci_session(
        &state,
        "td13-az-missing",
        None,
        Some("devops-project"),
        r#"[{"name":"agent","status":"idle"}]"#,
    );

    ci::poll_once(&state).await.expect("poll once");

    let (status, tool_message, polled_at, next_poll_at) = ci_row(&state, session_id, "azure");
    assert_eq!(status, "tool_unavailable");
    assert!(tool_message
        .as_deref()
        .is_some_and(|msg| msg.contains("brew install azure-cli")));
    assert!(DateTime::parse_from_rfc3339(&polled_at).is_ok());
    assert!(DateTime::parse_from_rfc3339(&next_poll_at).is_ok());
}

#[tokio::test]
async fn td_17_network_failure_records_error_without_crash() {
    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
echo "network unreachable" >&2
exit 1
"#,
    );
    let prev = set_env_var("SCMUX_GH_BIN", script.to_string_lossy().as_ref());

    let (state, _tmp) = build_state(ToolAvailability {
        gh_available: true,
        az_available: true,
    });
    let session_id = insert_ci_session(
        &state,
        "td17-network-fail",
        Some("acme/repo"),
        None,
        r#"[{"name":"agent","status":"active"}]"#,
    );

    let result = ci::poll_once(&state).await;
    restore_env_var("SCMUX_GH_BIN", prev);
    result.expect("poll once should not crash");

    let (status, tool_message, polled_at, next_poll_at) = ci_row(&state, session_id, "github");
    assert_eq!(status, "error");
    assert!(tool_message
        .as_deref()
        .is_some_and(|msg| msg.contains("network unreachable")));
    assert!(DateTime::parse_from_rfc3339(&polled_at).is_ok());
    assert!(DateTime::parse_from_rfc3339(&next_poll_at).is_ok());
}

#[tokio::test]
async fn td_18_auth_or_rate_limit_error_is_handled_gracefully() {
    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
echo "authentication failed for github.com" >&2
exit 1
"#,
    );
    let prev = set_env_var("SCMUX_GH_BIN", script.to_string_lossy().as_ref());

    let (state, _tmp) = build_state(ToolAvailability {
        gh_available: true,
        az_available: true,
    });
    let session_id = insert_ci_session(
        &state,
        "td18-auth-fail",
        Some("acme/repo"),
        None,
        r#"[{"name":"agent","status":"idle"}]"#,
    );

    let result = ci::poll_once(&state).await;
    restore_env_var("SCMUX_GH_BIN", prev);
    result.expect("poll once should not crash");

    let (status, tool_message, polled_at, next_poll_at) = ci_row(&state, session_id, "github");
    assert_eq!(status, "auth_error");
    assert!(tool_message
        .as_deref()
        .is_some_and(|msg| msg.to_ascii_lowercase().contains("authentication")));
    assert!(DateTime::parse_from_rfc3339(&polled_at).is_ok());
    assert!(DateTime::parse_from_rfc3339(&next_poll_at).is_ok());
}

#[tokio::test]
async fn td_10_td_11_poll_once_uses_active_and_idle_cadence() {
    let (state, _tmp) = build_state(ToolAvailability {
        gh_available: false,
        az_available: true,
    });
    let active_id = insert_ci_session(
        &state,
        "td10-active",
        Some("acme/repo"),
        None,
        r#"[{"name":"active-pane","status":"active"}]"#,
    );
    let idle_id = insert_ci_session(
        &state,
        "td11-idle",
        Some("acme/repo"),
        None,
        r#"[{"name":"idle-pane","status":"idle"}]"#,
    );

    ci::poll_once(&state).await.expect("poll once");

    let (_, _, active_polled, active_next) = ci_row(&state, active_id, "github");
    let active_polled = DateTime::parse_from_rfc3339(&active_polled).expect("active polled");
    let active_next = DateTime::parse_from_rfc3339(&active_next).expect("active next");
    let active_delta = active_next
        .signed_duration_since(active_polled)
        .num_seconds();
    assert!((55..=65).contains(&active_delta));

    let (_, _, idle_polled, idle_next) = ci_row(&state, idle_id, "github");
    let idle_polled = DateTime::parse_from_rfc3339(&idle_polled).expect("idle polled");
    let idle_next = DateTime::parse_from_rfc3339(&idle_next).expect("idle next");
    let idle_delta = idle_next.signed_duration_since(idle_polled).num_seconds();
    assert!((295..=305).contains(&idle_delta));
}
