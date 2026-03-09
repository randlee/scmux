use scmux_daemon::api;
use scmux_daemon::ci;
use scmux_daemon::config::{AtmConfig, Config, DaemonConfig, PollingConfig};
use scmux_daemon::db;
use scmux_daemon::definition_writer;
use scmux_daemon::tmux::PaneInfo;
use scmux_daemon::{AppState, SystemClock};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
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
        let host_id = definition_writer::ensure_local_host(&conn).expect("local host");
        conn.execute(
            "INSERT INTO hosts (name, address, ssh_user, api_port, is_local, last_seen)
             VALUES ('dgx-spark', '192.168.1.50', 'randlee', 7878, 0, datetime('now'))",
            [],
        )
        .expect("seed host");

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
                    enabled: true,
                    teams: vec!["scmux-dev".to_string()],
                    allow_shutdown: true,
                    socket_path: None,
                    stuck_minutes: Some(10),
                    stop_grace_secs: Some(1),
                },
            },
            reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
            runtime: std::sync::Mutex::new(scmux_daemon::runtime::RuntimeProjection::default()),
            ci_tools: ci::ToolAvailability::default(),
            clock: Arc::new(SystemClock),
            atm_available: std::sync::atomic::AtomicBool::new(false),
            last_api_access: std::sync::atomic::AtomicU64::new(0),
            started_at: std::time::Instant::now(),
            health: std::sync::Mutex::new(scmux_daemon::RuntimeHealth::default()),
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

    async fn create_armada(&self, name: &str) -> i64 {
        let response = self
            .client
            .post(format!("{}/editor/armadas", self.base_url))
            .json(&json!({ "name": name }))
            .send()
            .await
            .expect("create armada request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let db_conn = self.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM armadas WHERE name = ?1",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .expect("armada id")
    }

    async fn create_fleet(&self, armada_id: i64, name: &str) -> i64 {
        let response = self
            .client
            .post(format!("{}/editor/fleets", self.base_url))
            .json(&json!({ "armada_id": armada_id, "name": name }))
            .send()
            .await
            .expect("create fleet request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let db_conn = self.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM fleets WHERE armada_id = ?1 AND name = ?2",
                rusqlite::params![armada_id, name],
                |r| r.get(0),
            )
            .expect("fleet id")
    }

    async fn create_flotilla(&self, fleet_id: i64, name: &str) -> i64 {
        let response = self
            .client
            .post(format!("{}/editor/flotillas", self.base_url))
            .json(&json!({ "fleet_id": fleet_id, "name": name }))
            .send()
            .await
            .expect("create flotilla request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let db_conn = self.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM flotillas WHERE fleet_id = ?1 AND name = ?2",
                rusqlite::params![fleet_id, name],
                |r| r.get(0),
            )
            .expect("flotilla id")
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
    assert!(body["uptime_secs"].as_u64().is_some());
    assert!(body["session_count"].as_i64().is_some());
    assert!(body["db_path"].as_str().is_some());
    assert!(body["version"].as_str().is_some());
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
        let panes = vec![PaneInfo {
            index: 0,
            name: "pane-0".to_string(),
            status: "active".to_string(),
            last_activity: "now".to_string(),
            current_command: "bash".to_string(),
        }];
        let mut live = std::collections::HashMap::new();
        live.insert("alpha".to_string(), panes);
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        runtime.apply_tmux_snapshot(
            &["alpha".to_string()],
            &live,
            &std::collections::HashMap::new(),
            &chrono::Utc::now().to_rfc3339(),
        );
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
async fn t_a_04_get_sessions_name_returns_200_with_config() {
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
    assert!(body.get("recent_events").is_none());
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
async fn t_lc_01_post_sessions_name_start_launches_tmux_from_config() {
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
}

#[tokio::test]
async fn t_lc_06_start_failure_returns_500_and_keeps_session_stopped() {
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

    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["code"], "start_failed");
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("tmuxp"));

    let sessions = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(sessions.status(), reqwest::StatusCode::OK);
    let rows: Vec<Value> = sessions.json().await.expect("json");
    assert_eq!(rows[0]["status"], "stopped");
}

#[tokio::test]
async fn t_lc_03_stop_grace_then_hard_stop_when_atm_send_is_stubbed() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let _guard = env_lock().lock().await;
    let marker = tempfile::NamedTempFile::new().expect("marker file");
    let marker_path = marker.path().to_string_lossy().to_string();

    let script = write_script(&format!(
        r#"#!/bin/sh
if [ "$1" = "kill-session" ]; then
  echo "kill" >> "{marker_path}"
  exit 0
fi
if [ "$1" = "list-sessions" ]; then
  echo "alpha"
  exit 0
fi
exit 1
"#,
    ));
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", script.to_string_lossy().as_ref());
    let started = Instant::now();

    let response = h
        .client
        .post(format!("{}/sessions/alpha/stop", h.base_url))
        .send()
        .await
        .expect("stop request");
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], true);
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("after graceful timeout"));
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "stop path should include grace sleep before hard-stop"
    );

    let marker_log = std::fs::read_to_string(marker.path()).expect("read marker");
    assert!(
        marker_log.contains("kill\n"),
        "expected tmux hard-stop after grace timeout, got: {marker_log:?}"
    );
}

#[tokio::test]
async fn t_ed_01_create_editor_hierarchy_and_crew_bundle() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Work Dev").await;
    let fleet_id = h.create_fleet(armada_id, "Core").await;
    let flotilla_id = h.create_flotilla(fleet_id, "Backend").await;

    let response = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-alpha",
            "crew_ulid": "01JCREWALPHA00000000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["prompts/arch-startup.md", "prompts/pm-startup.md"]
                },
                {
                    "member_id": "arch-cmux",
                    "role": "mate",
                    "ai_provider": "codex",
                    "model": "codex-high",
                    "startup_prompts": ["prompts/arch-cmux-startup.md"]
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "repo_url": "git@github.com:randlee/scmux.git",
                    "branch_ref": "feature/demo",
                    "root_path": "/tmp/scmux",
                    "config_json": { "session_name": "crew-alpha" }
                }
            ],
            "placement": {
                "armada_id": armada_id,
                "fleet_id": fleet_id,
                "flotilla_id": flotilla_id
            }
        }))
        .send()
        .await
        .expect("create crew bundle");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let db_conn = h.state.db.lock().expect("db lock");
    let crews: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crews WHERE crew_name = 'crew-alpha'",
            [],
            |r| r.get(0),
        )
        .expect("crew count");
    let members: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crew_members cm JOIN crews c ON c.id = cm.crew_id WHERE c.crew_name = 'crew-alpha'",
            [],
            |r| r.get(0),
        )
        .expect("member count");
    let variants: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crew_variants cv JOIN crews c ON c.id = cv.crew_id WHERE c.crew_name = 'crew-alpha'",
            [],
            |r| r.get(0),
        )
        .expect("variant count");
    let refs_count: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crew_refs cr JOIN crews c ON c.id = cr.crew_id WHERE c.crew_name = 'crew-alpha'",
            [],
            |r| r.get(0),
        )
        .expect("ref count");

    assert_eq!(crews, 1);
    assert_eq!(members, 2);
    assert_eq!(variants, 1);
    assert_eq!(refs_count, 1);
}

#[tokio::test]
async fn t_ed_02_clone_move_and_unlink_crew_ref_with_reference_count_delete() {
    let h = ApiHarness::new().await;
    let armada_a = h.create_armada("A").await;
    let fleet_a = h.create_fleet(armada_a, "Fleet-A").await;
    let armada_b = h.create_armada("B").await;
    let fleet_b = h.create_fleet(armada_b, "Fleet-B").await;

    let create = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-src",
            "crew_ulid": "01JCREWSRC0000000000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["a.md"]
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "root_path": "/tmp/crew-src"
                }
            ],
            "placement": { "armada_id": armada_a, "fleet_id": fleet_a }
        }))
        .send()
        .await
        .expect("create source crew");
    assert_eq!(create.status(), reqwest::StatusCode::OK);

    let (source_crew_id, source_ref_id): (i64, i64) = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT c.id, r.id FROM crews c JOIN crew_refs r ON r.crew_id = c.id WHERE c.crew_name = 'crew-src' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("source ids")
    };

    let clone = h
        .client
        .post(format!(
            "{}/editor/crews/{}/clone",
            h.base_url, source_crew_id
        ))
        .json(&json!({
            "crew_name": "crew-clone",
            "crew_ulid": "01JCREWCLONE00000000000000",
            "placement": { "armada_id": armada_b, "fleet_id": fleet_b }
        }))
        .send()
        .await
        .expect("clone crew");
    assert_eq!(clone.status(), reqwest::StatusCode::OK);

    let move_ref = h
        .client
        .post(format!(
            "{}/editor/crew-refs/{}/move",
            h.base_url, source_ref_id
        ))
        .json(&json!({
            "armada_id": armada_b,
            "fleet_id": fleet_b
        }))
        .send()
        .await
        .expect("move crew ref");
    assert_eq!(move_ref.status(), reqwest::StatusCode::OK);

    let unlink = h
        .client
        .delete(format!("{}/editor/crew-refs/{}", h.base_url, source_ref_id))
        .send()
        .await
        .expect("unlink crew ref");
    assert_eq!(unlink.status(), reqwest::StatusCode::OK);

    let db_conn = h.state.db.lock().expect("db lock");
    let source_exists: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crews WHERE id = ?1",
            rusqlite::params![source_crew_id],
            |r| r.get(0),
        )
        .expect("source exists count");
    let clone_exists: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM crews WHERE crew_name = 'crew-clone'",
            [],
            |r| r.get(0),
        )
        .expect("clone exists count");
    assert_eq!(source_exists, 0);
    assert_eq!(clone_exists, 1);
}

#[tokio::test]
async fn t_ed_03_running_crew_blocks_roster_patch() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Run").await;
    let fleet_id = h.create_fleet(armada_id, "Fleet").await;

    let create = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-running",
            "crew_ulid": "01JCREWRUN000000000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["a.md"]
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "root_path": "/tmp/crew-running"
                }
            ],
            "placement": { "armada_id": armada_id, "fleet_id": fleet_id }
        }))
        .send()
        .await
        .expect("create crew-running");
    assert_eq!(create.status(), reqwest::StatusCode::OK);

    let crew_id: i64 = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM crews WHERE crew_name = 'crew-running'",
                [],
                |r| r.get(0),
            )
            .expect("crew id")
    };
    {
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        runtime.mark_starting("crew-running");
    }
    let response = h
        .client
        .patch(format!("{}/editor/crews/{}", h.base_url, crew_id))
        .json(&json!({
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["updated.md"]
                }
            ]
        }))
        .send()
        .await
        .expect("patch running crew");

    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["code"], "running_edit_forbidden");
}

#[tokio::test]
async fn t_ed_04_invalid_roster_patch_is_atomic() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Atomic").await;
    let fleet_id = h.create_fleet(armada_id, "Fleet").await;

    let create = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-atomic",
            "crew_ulid": "01JCREWATOMIC000000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["a.md"]
                },
                {
                    "member_id": "arch-cmux",
                    "role": "mate",
                    "ai_provider": "codex",
                    "model": "codex-high",
                    "startup_prompts": ["b.md"]
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "root_path": "/tmp/crew-atomic"
                }
            ],
            "placement": { "armada_id": armada_id, "fleet_id": fleet_id }
        }))
        .send()
        .await
        .expect("create crew-atomic");
    assert_eq!(create.status(), reqwest::StatusCode::OK);

    let crew_id: i64 = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM crews WHERE crew_name = 'crew-atomic'",
                [],
                |r| r.get(0),
            )
            .expect("crew id")
    };

    let before_count: i64 = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT COUNT(*) FROM crew_members WHERE crew_id = ?1",
                rusqlite::params![crew_id],
                |r| r.get(0),
            )
            .expect("before count")
    };
    assert_eq!(before_count, 2);

    let response = h
        .client
        .patch(format!("{}/editor/crews/{}", h.base_url, crew_id))
        .json(&json!({
            "members": [
                {
                    "member_id": "arch-cmux",
                    "role": "mate",
                    "ai_provider": "codex",
                    "model": "codex-high",
                    "startup_prompts": ["only-mate.md"]
                }
            ]
        }))
        .send()
        .await
        .expect("invalid patch request");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

    let after_count: i64 = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT COUNT(*) FROM crew_members WHERE crew_id = ?1",
                rusqlite::params![crew_id],
                |r| r.get(0),
            )
            .expect("after count")
    };
    assert_eq!(after_count, 2);
}

#[tokio::test]
async fn t_ed_05_clone_armada_and_fleet_endpoints() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Source Armada").await;
    let fleet_id = h.create_fleet(armada_id, "Source Fleet").await;

    let armada_clone = h
        .client
        .post(format!("{}/editor/armadas/{}/clone", h.base_url, armada_id))
        .json(&json!({ "name": "Cloned Armada" }))
        .send()
        .await
        .expect("clone armada");
    assert_eq!(armada_clone.status(), reqwest::StatusCode::OK);

    let db_conn = h.state.db.lock().expect("db lock");
    let cloned_armada_id: i64 = db_conn
        .query_row(
            "SELECT id FROM armadas WHERE name = 'Cloned Armada'",
            [],
            |r| r.get(0),
        )
        .expect("cloned armada id");
    drop(db_conn);

    let fleet_clone = h
        .client
        .post(format!("{}/editor/fleets/{}/clone", h.base_url, fleet_id))
        .json(&json!({
            "armada_id": cloned_armada_id,
            "name": "Cloned Fleet"
        }))
        .send()
        .await
        .expect("clone fleet");
    assert_eq!(fleet_clone.status(), reqwest::StatusCode::OK);

    let db_conn = h.state.db.lock().expect("db lock");
    let exists: i64 = db_conn
        .query_row(
            "SELECT COUNT(*) FROM fleets WHERE armada_id = ?1 AND name = 'Cloned Fleet'",
            rusqlite::params![cloned_armada_id],
            |r| r.get(0),
        )
        .expect("cloned fleet count");
    assert_eq!(exists, 1);
}

#[tokio::test]
async fn t_ed_06_create_crew_rejects_empty_startup_prompts() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Prompts").await;
    let fleet_id = h.create_fleet(armada_id, "Fleet").await;

    let response = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-empty-prompts",
            "crew_ulid": "01JEMPTYPROMPTS00000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": []
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "root_path": "/tmp/crew-empty-prompts"
                }
            ],
            "placement": { "armada_id": armada_id, "fleet_id": fleet_id }
        }))
        .send()
        .await
        .expect("create invalid crew");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["code"], "validation_error");
}

#[tokio::test]
async fn t_ed_07_create_crew_rejects_empty_variants() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Variants").await;
    let fleet_id = h.create_fleet(armada_id, "Fleet").await;

    let response = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-no-variants",
            "crew_ulid": "01JNOVARIANTS000000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["lead.md"]
                }
            ],
            "variants": [],
            "placement": { "armada_id": armada_id, "fleet_id": fleet_id }
        }))
        .send()
        .await
        .expect("create invalid crew");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["code"], "validation_error");
}

#[tokio::test]
async fn t_ed_08_running_guard_uses_runtime_projection() {
    let h = ApiHarness::new().await;
    let armada_id = h.create_armada("Runtime").await;
    let fleet_id = h.create_fleet(armada_id, "Fleet").await;

    let create = h
        .client
        .post(format!("{}/editor/crews", h.base_url))
        .json(&json!({
            "crew_name": "crew-runtime-guard",
            "crew_ulid": "01JRUNTIMEGUARD0000000000",
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["a.md"]
                }
            ],
            "variants": [
                {
                    "host_id": h.state.host_id,
                    "root_path": "/tmp/crew-runtime-guard"
                }
            ],
            "placement": { "armada_id": armada_id, "fleet_id": fleet_id }
        }))
        .send()
        .await
        .expect("create crew");
    assert_eq!(create.status(), reqwest::StatusCode::OK);

    let crew_id: i64 = {
        let db_conn = h.state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT id FROM crews WHERE crew_name = 'crew-runtime-guard'",
                [],
                |r| r.get(0),
            )
            .expect("crew id")
    };
    {
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        runtime.mark_starting("crew-runtime-guard");
    }

    let response = h
        .client
        .patch(format!("{}/editor/crews/{}", h.base_url, crew_id))
        .json(&json!({
            "members": [
                {
                    "member_id": "team-lead",
                    "role": "captain",
                    "ai_provider": "claude",
                    "model": "claude-opus",
                    "startup_prompts": ["updated.md"]
                }
            ]
        }))
        .send()
        .await
        .expect("patch crew");

    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["code"], "running_edit_forbidden");
}

#[tokio::test]
#[cfg(target_os = "macos")]
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
}

#[tokio::test]
#[cfg(not(target_os = "macos"))]
async fn t_a_09_post_sessions_name_jump_returns_ok_false_when_not_macos() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;

    let response = h
        .client
        .post(format!("{}/sessions/alpha/jump", h.base_url))
        .json(&json!({ "terminal": "iterm2" }))
        .send()
        .await
        .expect("jump request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    assert_eq!(body["ok"], false);
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("only supported on macOS"));
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
        "config_json": {
            "session_name": "alpha",
            "panes": [
                { "name": "agent", "command": "sleep 1", "atm_agent": "agent", "atm_team": "scmux-dev" }
            ]
        },
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
        drop(db);
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        runtime.upsert_ci(
            "alpha",
            session_id,
            scmux_daemon::runtime::CiRuntimeSummary {
                provider: "github".to_string(),
                status: "ok".to_string(),
                data_json: Some(serde_json::json!({
                    "prs": [{"number": 123, "title": "feat: test"}],
                    "runs": [{"status": "completed"}]
                })),
                tool_message: None,
                polled_at: Some(chrono::Utc::now().to_rfc3339()),
                next_poll_at: Some(
                    (chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
                ),
            },
            chrono::Utc::now() + chrono::Duration::minutes(1),
        );
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
        drop(db);
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        runtime.upsert_ci(
            "alpha",
            session_id,
            scmux_daemon::runtime::CiRuntimeSummary {
                provider: "github".to_string(),
                status: "tool_unavailable".to_string(),
                data_json: None,
                tool_message: Some("Install gh CLI: brew install gh".to_string()),
                polled_at: Some(chrono::Utc::now().to_rfc3339()),
                next_poll_at: Some(
                    (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
                ),
            },
            chrono::Utc::now() + chrono::Duration::minutes(5),
        );
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
async fn t_atm_01_get_sessions_includes_atm_null_for_non_atm_sessions() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    h.state.atm_available.store(true, Ordering::Relaxed);

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert!(body[0]["atm"].is_null());
}

#[tokio::test]
async fn t_atm_02_get_sessions_includes_atm_state_when_available() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    {
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        let mut live = std::collections::HashMap::new();
        live.insert(
            "alpha".to_string(),
            vec![PaneInfo {
                index: 0,
                name: "agent".to_string(),
                status: "idle".to_string(),
                last_activity: "now".to_string(),
                current_command: "sleep 1".to_string(),
            }],
        );
        let mut pane_configs = std::collections::HashMap::new();
        pane_configs.insert(
            "alpha".to_string(),
            vec![scmux_daemon::runtime::ConfiguredPane {
                name: Some("agent".to_string()),
                command: Some("sleep 1".to_string()),
                atm_team: Some("scmux-dev".to_string()),
                atm_agent: Some("agent".to_string()),
            }],
        );
        runtime.apply_tmux_snapshot(
            &["alpha".to_string()],
            &live,
            &pane_configs,
            &chrono::Utc::now().to_rfc3339(),
        );
        runtime.apply_atm_updates(vec![scmux_daemon::runtime::AtmRuntimeUpdate {
            team: "scmux-dev".to_string(),
            agent: "agent".to_string(),
            state: "active".to_string(),
            last_transition: Some("2026-03-08T00:00:00Z".to_string()),
        }]);
    }
    h.state.atm_available.store(true, Ordering::Relaxed);

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["atm"]["state"], "active");
    assert_eq!(body[0]["atm"]["last_transition"], "2026-03-08T00:00:00Z");
}

#[tokio::test]
async fn t_atm_03_unreachable_atm_returns_null_without_error() {
    let h = ApiHarness::new().await;
    h.create_session("alpha").await;
    {
        let mut runtime = h.state.runtime.lock().expect("runtime lock");
        let mut live = std::collections::HashMap::new();
        live.insert(
            "alpha".to_string(),
            vec![PaneInfo {
                index: 0,
                name: "agent".to_string(),
                status: "idle".to_string(),
                last_activity: "now".to_string(),
                current_command: "sleep 1".to_string(),
            }],
        );
        let mut pane_configs = std::collections::HashMap::new();
        pane_configs.insert(
            "alpha".to_string(),
            vec![scmux_daemon::runtime::ConfiguredPane {
                name: Some("agent".to_string()),
                command: Some("sleep 1".to_string()),
                atm_team: Some("scmux-dev".to_string()),
                atm_agent: Some("agent".to_string()),
            }],
        );
        runtime.apply_tmux_snapshot(
            &["alpha".to_string()],
            &live,
            &pane_configs,
            &chrono::Utc::now().to_rfc3339(),
        );
        runtime.apply_atm_updates(vec![scmux_daemon::runtime::AtmRuntimeUpdate {
            team: "scmux-dev".to_string(),
            agent: "agent".to_string(),
            state: "stuck".to_string(),
            last_transition: Some("2026-03-08T00:00:00Z".to_string()),
        }]);
    }
    h.state.atm_available.store(false, Ordering::Relaxed);

    let response = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("sessions request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Vec<Value> = response.json().await.expect("json");
    assert_eq!(body.len(), 1);
    assert!(body[0]["atm"].is_null());
}

#[tokio::test]
#[ignore = "perf-gate: run in --release CI job"]
async fn td_23_get_sessions_latency_under_100ms_at_50_sessions() {
    let h = ApiHarness::new().await;
    for idx in 0..50 {
        h.create_session(&format!("perf-{idx}")).await;
    }

    let warmup = h
        .client
        .get(format!("{}/sessions", h.base_url))
        .send()
        .await
        .expect("warm-up sessions request");
    assert_eq!(warmup.status(), reqwest::StatusCode::OK);

    let mut samples = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        let response = h
            .client
            .get(format!("{}/sessions", h.base_url))
            .send()
            .await
            .expect("sessions request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        samples.push(started.elapsed());
    }

    samples.sort();
    let p95_index = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    let p95 = samples[p95_index];
    assert!(
        p95 < Duration::from_millis(100),
        "GET /sessions p95 exceeded 100ms at 50 sessions: {:?}",
        p95
    );
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
