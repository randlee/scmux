use scmux::client::{ApiClient, JumpRequest};
use scmux_daemon::api;
use scmux_daemon::ci;
use scmux_daemon::config::{AtmConfig, Config, DaemonConfig, PollingConfig};
use scmux_daemon::db;
use scmux_daemon::{AppState, SystemClock};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
use std::io::Write;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
#[cfg(target_os = "macos")]
use tokio::sync::Mutex;

#[cfg(target_os = "macos")]
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(target_os = "macos")]
fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

struct CliE2eHarness {
    base_url: String,
    client: reqwest::Client,
    _tmp: TempDir,
    shutdown: Option<oneshot::Sender<()>>,
}

impl CliE2eHarness {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("scmux-cli-e2e.db");
        let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
        let host_id = db::ensure_local_host(&conn).expect("local host");

        let state = Arc::new(AppState {
            db: std::sync::Mutex::new(conn),
            db_path: db_path.to_string_lossy().to_string(),
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
                atm: AtmConfig {
                    socket_path: None,
                    stuck_minutes: Some(10),
                    stop_grace_secs: None,
                },
                hosts: Vec::new(),
            },
            reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
            runtime: std::sync::Mutex::new(scmux_daemon::runtime::RuntimeProjection::default()),
            ci_tools: ci::ToolAvailability::default(),
            clock: Arc::new(SystemClock),
            atm_available: std::sync::atomic::AtomicBool::new(false),
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
            _tmp: tmp,
            shutdown: Some(tx),
        }
    }

    async fn create_session(&self, name: &str) {
        let payload = json!({
            "name": name,
            "project": "e2e",
            "config_json": {
                "session_name": name,
                "panes": [
                    { "name": "agent", "command": "sleep 1", "atm_agent": "agent", "atm_team": "scmux-dev" }
                ]
            },
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
}

impl Drop for CliE2eHarness {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[cfg(target_os = "macos")]
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

#[tokio::test]
async fn t_e_10_scmux_list_matches_daemon_sessions() {
    let h = CliE2eHarness::new().await;
    h.create_session("te10-alpha").await;
    h.create_session("te10-beta").await;

    let sessions: Vec<serde_json::Value> = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request")
        .json()
        .await
        .expect("sessions json");
    let names = sessions
        .iter()
        .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();

    let client = ApiClient::new(h.base_url.clone());
    let cli_sessions = client.list_sessions().await.expect("list sessions");
    let cli_names = cli_sessions
        .iter()
        .map(|session| session.name.as_str())
        .collect::<Vec<_>>();

    for name in names {
        assert!(
            cli_names.contains(&name),
            "scmux list missing session '{name}'"
        );
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn t_e_11_scmux_jump_launches_via_daemon() {
    let h = CliE2eHarness::new().await;
    h.create_session("te11-jump").await;

    let _guard = env_lock().lock().await;
    let script = write_script("#!/bin/sh\nexit 0\n");
    // SAFETY: test-only env mutation guarded by async mutex.
    unsafe { std::env::set_var("SCMUX_OSASCRIPT_BIN", script.to_string_lossy().to_string()) };

    let client = ApiClient::new(h.base_url.clone());
    let action = client
        .jump_session(
            "te11-jump",
            &JumpRequest {
                terminal: None,
                host_id: None,
            },
        )
        .await
        .expect("jump request");

    // SAFETY: test teardown under lock.
    unsafe { std::env::remove_var("SCMUX_OSASCRIPT_BIN") };
    assert!(action.ok, "jump action should succeed on macOS");
}

#[cfg(not(target_os = "macos"))]
#[tokio::test]
async fn t_e_11_scmux_jump_returns_clear_non_macos_error() {
    let h = CliE2eHarness::new().await;
    h.create_session("te11-jump").await;

    let client = ApiClient::new(h.base_url.clone());
    let action = client
        .jump_session(
            "te11-jump",
            &JumpRequest {
                terminal: None,
                host_id: None,
            },
        )
        .await
        .expect("jump request");

    assert!(!action.ok);
    assert!(action.message.contains("only supported on macOS"));
}
