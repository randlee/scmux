use scmux_daemon::api;
use scmux_daemon::config::{Config, DaemonConfig, PollingConfig};
use scmux_daemon::db;
use scmux_daemon::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::oneshot;

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

#[tokio::test]
async fn t_a_01_get_health_returns_status() {
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
}

#[tokio::test]
async fn t_a_02_get_sessions_empty_on_fresh_db() {
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
async fn t_a_03_post_sessions_creates_session() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

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
async fn t_a_04_get_sessions_returns_created_session() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "alpha");
}

#[tokio::test]
async fn t_a_05_get_session_detail_returns_config_and_events() {
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
}

#[tokio::test]
async fn t_a_06_get_session_not_found_returns_404() {
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
async fn t_a_07_get_dashboard_config_returns_hosts_and_settings() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/dashboard-config.json", h.base_url))
        .send()
        .await
        .expect("dashboard config request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["default_terminal"], "iterm2");
    assert_eq!(body["poll_interval_ms"], 15000);
    assert!(!body["hosts"].as_array().expect("hosts array").is_empty());
}

#[tokio::test]
async fn t_a_08_post_start_logs_event() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .post(format!("{}/sessions/alpha/start", h.base_url))
        .send()
        .await
        .expect("start request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert!(body["ok"].is_boolean());
    assert!(body["message"].is_string());
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_09_post_stop_logs_event() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .post(format!("{}/sessions/alpha/stop", h.base_url))
        .send()
        .await
        .expect("stop request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert!(body["ok"].is_boolean());
    assert!(body["message"].is_string());
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_10_post_jump_returns_action_response_and_logs_event() {
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
    assert!(h.session_event_count("alpha") >= 1);
}

#[tokio::test]
async fn t_a_11_unknown_route_returns_404() {
    let h = ApiHarness::new().await;
    let response = h
        .client
        .get(format!("{}/not-found", h.base_url))
        .send()
        .await
        .expect("unknown route request");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn t_a_12_patch_session_updates_cron_schedule() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .patch(format!("{}/sessions/alpha", h.base_url))
        .json(&json!({ "cron_schedule": "0 0 12 1 1 *" }))
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
    assert_eq!(cron.as_deref(), Some("0 0 12 1 1 *"));
}

#[tokio::test]
async fn t_a_13_delete_session_soft_deletes_enabled_flag() {
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
async fn t_a_14_get_hosts_returns_reachability_flag() {
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
