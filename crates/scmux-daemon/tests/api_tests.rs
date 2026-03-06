use scmux_daemon::api;
use scmux_daemon::ci;
use scmux_daemon::config::{Config, DaemonConfig, PollingConfig};
use scmux_daemon::db;
use scmux_daemon::AppState;
use serde_json::{json, Value};
use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use tempfile::TempDir;
use tokio::sync::{oneshot, Mutex};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

struct ApiHarness {
    base_url: String,
    client: reqwest::Client,
    state: Arc<AppState>,
    _tmp: TempDir,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ApiHarness {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("scmux-test.db");
        let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
        let host_id = db::ensure_local_host(&conn).expect("local host");
        conn.execute(
            "INSERT INTO hosts (name, address, ssh_user, api_port, is_local, last_seen)
             VALUES ('dgx-spark', '192.168.1.50', 'randlee', 7700, 0, datetime('now'))",
            [],
        )
        .expect("seed host");

        let state = Arc::new(AppState {
            db: std::sync::Mutex::new(conn),
            host_id,
            config: Config {
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
            },
            reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
            ci_tools: ci::ToolAvailability::default(),
            last_api_access: std::sync::atomic::AtomicU64::new(0),
            started_at: std::time::Instant::now(),
        });

        let router = api::router(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });

        Self {
            base_url: format!("http://{addr}"),
            client: reqwest::Client::new(),
            state,
            _tmp: tmp,
            shutdown: Some(tx),
        }
    }

    async fn create_session(&self, name: &str) {
        let payload = json!({
            "name": name,
            "project": "demo",
            "config_json": { "session_name": name },
            "auto_start": false
        });
        let response = self
            .client
            .post(format!("{}/sessions", self.base_url))
            .json(&payload)
            .send()
            .await
            .expect("create session request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }

    fn session_event_count(&self, name: &str) -> i64 {
        let db = self.state.db.lock().expect("db lock");
        db.query_row(
            "SELECT COUNT(*)
             FROM session_events se
             INNER JOIN sessions s ON s.id = se.session_id
             WHERE s.name = ?1",
            [name],
            |r| r.get(0),
        )
        .expect("event count")
    }
}

impl Drop for ApiHarness {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
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

#[tokio::test]
async fn t_a_01_get_health_returns_200_with_correct_fields() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/health", h.base_url))
        .send()
        .await
        .expect("health request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["status"], "ok");
    assert!(body["host_id"].as_i64().is_some());
    assert!(body["sessions_running"].as_i64().is_some());
    assert!(body["polled_at"].as_str().is_some());
}

#[tokio::test]
async fn t_a_02_get_sessions_returns_empty_array_when_no_sessions() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert!(body.is_empty());
}

#[tokio::test]
async fn t_a_03_get_sessions_returns_sessions_with_correct_status_and_panes() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    {
        let db = h.state.db.lock().expect("db lock");
        let session_id: i64 = db
            .query_row("SELECT id FROM sessions WHERE name = 'alpha'", [], |r| {
                r.get(0)
            })
            .expect("session id");
        db.execute(
            "INSERT INTO session_status (session_id, status, panes_json, polled_at)
             VALUES (?1, 'running', ?2, datetime('now'))",
            rusqlite::params![
                session_id,
                r#"[{"index":0,"name":"pane-0","status":"active","last_activity":"now","current_command":"bash"}]"#
            ],
        )
        .expect("insert status");
    }

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert!(body[0]["host_id"].as_i64().is_some());
    assert_eq!(body[0]["status"], "running");
    assert!(body[0]["panes"].is_array());
    assert!(!body[0]["panes"].as_array().expect("panes").is_empty());
}

#[tokio::test]
async fn t_a_04_get_sessions_name_returns_200_with_config_and_events() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .get(format!("{}/sessions/alpha", h.base_url))
        .send()
        .await
        .expect("detail request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["name"], "alpha");
    assert_eq!(body["config_json"]["session_name"], "alpha");
    assert!(body["recent_events"].is_array());
}

#[tokio::test]
async fn t_a_05_get_sessions_name_returns_404_for_unknown_session() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/sessions/missing", h.base_url))
        .send()
        .await
        .expect("detail request");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn t_a_06_post_sessions_name_start_returns_ok_true_and_logs_event() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 0\n");
    let prev = set_env_var("SCMUX_TMUXP_BIN", script.to_string_lossy().as_ref());

    let response = h
        .client
        .post(format!("{}/sessions/alpha/start", h.base_url))
        .send()
        .await
        .expect("start request");
    restore_env_var("SCMUX_TMUXP_BIN", prev);

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], true);
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_07_post_sessions_name_start_returns_ok_false_on_tmuxp_failure() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\necho \"tmuxp failed\" 1>&2\nexit 1\n");
    let prev = set_env_var("SCMUX_TMUXP_BIN", script.to_string_lossy().as_ref());

    let response = h
        .client
        .post(format!("{}/sessions/alpha/start", h.base_url))
        .send()
        .await
        .expect("start request");
    restore_env_var("SCMUX_TMUXP_BIN", prev);

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], false);
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("tmuxp"));
}

#[tokio::test]
async fn t_a_08_post_sessions_name_stop_returns_ok_true_and_logs_event() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let _guard = env_lock().lock().await;
    let script = write_script(
        r#"#!/bin/sh
if [ "$1" = "kill-session" ]; then
  exit 0
fi
exit 1
"#,
    );
    let prev = set_env_var("SCMUX_TMUX_BIN", script.to_string_lossy().as_ref());

    let response = h
        .client
        .post(format!("{}/sessions/alpha/stop", h.base_url))
        .send()
        .await
        .expect("stop request");
    restore_env_var("SCMUX_TMUX_BIN", prev);

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], true);
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_09_post_sessions_name_jump_returns_ok_true_when_iterm2_launched() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 0\n");
    let prev = set_env_var("SCMUX_OSASCRIPT_BIN", script.to_string_lossy().as_ref());

    let response = h
        .client
        .post(format!("{}/sessions/alpha/jump", h.base_url))
        .json(&json!({ "terminal": "iterm2" }))
        .send()
        .await
        .expect("jump request");
    restore_env_var("SCMUX_OSASCRIPT_BIN", prev);

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], true);
    assert_eq!(body["message"], "launched iTerm2");
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_10_post_sessions_name_jump_returns_ok_false_when_terminal_unavailable() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .post(format!("{}/sessions/alpha/jump", h.base_url))
        .json(&json!({ "terminal": "invalid-terminal" }))
        .send()
        .await
        .expect("jump request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], false);
}

#[tokio::test]
async fn t_a_11_post_sessions_add_creates_session_in_sqlite() {
    let h = ApiHarness::new().await;
    let payload = json!({
        "name": "alpha",
        "project": "demo",
        "config_json": { "session_name": "alpha" },
        "auto_start": false
    });
    let response = h
        .client
        .post(format!("{}/sessions", h.base_url))
        .json(&payload)
        .send()
        .await
        .expect("create session request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let db = h.state.db.lock().expect("db lock");
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE name = 'alpha'",
            [],
            |r| r.get(0),
        )
        .expect("query session");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn t_a_12_patch_sessions_name_updates_cron_schedule() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .patch(format!("{}/sessions/alpha", h.base_url))
        .json(&json!({ "cron_schedule": "0 9 * * 1-5" }))
        .send()
        .await
        .expect("patch request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let db = h.state.db.lock().expect("db lock");
    let cron: Option<String> = db
        .query_row(
            "SELECT cron_schedule FROM sessions WHERE name = 'alpha'",
            [],
            |r| r.get(0),
        )
        .expect("query cron");
    assert_eq!(cron.as_deref(), Some("0 9 * * 1-5"));
}

#[tokio::test]
async fn t_a_13_delete_sessions_name_disables_session() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .delete(format!("{}/sessions/alpha", h.base_url))
        .send()
        .await
        .expect("delete request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let db = h.state.db.lock().expect("db lock");
    let enabled: bool = db
        .query_row(
            "SELECT enabled FROM sessions WHERE name = 'alpha'",
            [],
            |r| r.get(0),
        )
        .expect("query enabled");
    assert!(!enabled);
}

#[tokio::test]
async fn t_a_14_get_hosts_returns_all_hosts_with_reachability_flag() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/hosts", h.base_url))
        .send()
        .await
        .expect("hosts request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert!(body.len() >= 2);
    assert!(body.iter().all(|row| row["reachable"].is_boolean()));
}

#[tokio::test]
async fn t_a_15_get_sessions_includes_ci_summary_payload() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    {
        let db = h.state.db.lock().expect("db lock");
        let session_id: i64 = db
            .query_row("SELECT id FROM sessions WHERE name = 'alpha'", [], |r| {
                r.get(0)
            })
            .expect("session id");
        db.execute(
            "INSERT INTO session_ci (session_id, provider, status, data_json, tool_message, polled_at, next_poll_at)
             VALUES (?1, 'github', 'ok', ?2, NULL, datetime('now'), datetime('now', '+1 minute'))",
            rusqlite::params![
                session_id,
                r#"{"prs":[{"number":123,"title":"feat: test"}],"runs":[{"status":"completed"}]}"#
            ],
        )
        .expect("insert session ci");
    }

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert!(body[0]["session_ci"].is_array());
    assert_eq!(body[0]["session_ci"][0]["provider"], "github");
    assert_eq!(body[0]["session_ci"][0]["status"], "ok");
    assert_eq!(
        body[0]["session_ci"][0]["data_json"]["prs"][0]["number"],
        123
    );
}

#[tokio::test]
async fn t_a_16_get_session_detail_includes_ci_summary_payload() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    {
        let db = h.state.db.lock().expect("db lock");
        let session_id: i64 = db
            .query_row("SELECT id FROM sessions WHERE name = 'alpha'", [], |r| {
                r.get(0)
            })
            .expect("session id");
        db.execute(
            "INSERT INTO session_ci (session_id, provider, status, data_json, tool_message, polled_at, next_poll_at)
             VALUES (?1, 'github', 'tool_unavailable', NULL, 'Install gh CLI: brew install gh', datetime('now'), datetime('now', '+5 minute'))",
            rusqlite::params![session_id],
        )
        .expect("insert session ci");
    }

    let response = h
        .client
        .get(format!("{}/sessions/alpha", h.base_url))
        .send()
        .await
        .expect("detail request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert!(body["session_ci"].is_array());
    assert_eq!(body["session_ci"][0]["provider"], "github");
    assert_eq!(body["session_ci"][0]["status"], "tool_unavailable");
    assert!(body["session_ci"][0]["tool_message"]
        .as_str()
        .unwrap_or_default()
        .contains("brew install gh"));
}

#[tokio::test]
async fn route_not_found_returns_404() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/not-found", h.base_url))
        .send()
        .await
        .expect("unknown route request");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}
