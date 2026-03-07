use chrono::{Datelike, Timelike, Utc};
use scmux_daemon::api;
use scmux_daemon::ci;
use scmux_daemon::config::{Config, DaemonConfig, PollingConfig};
use scmux_daemon::db;
use scmux_daemon::scheduler;
use scmux_daemon::{AppState, Clock, SystemClock};
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

struct FixedClock {
    now: chrono::DateTime<Utc>,
}

impl Clock for FixedClock {
    fn now_utc(&self) -> chrono::DateTime<Utc> {
        self.now
    }
}

struct E2eHarness {
    base_url: String,
    client: reqwest::Client,
    state: Arc<AppState>,
    _tmp: TempDir,
    shutdown: Option<oneshot::Sender<()>>,
}

impl E2eHarness {
    async fn new() -> Self {
        Self::new_with_clock(Arc::new(SystemClock)).await
    }

    async fn new_with_clock(clock: Arc<dyn Clock>) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("scmux-e2e.db");
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
                hosts: Vec::new(),
            },
            reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
            ci_tools: ci::ToolAvailability::default(),
            clock,
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

    async fn create_session(&self, name: &str, auto_start: bool, cron_schedule: Option<&str>) {
        let payload = json!({
            "name": name,
            "project": "e2e",
            "config_json": { "session_name": name },
            "auto_start": auto_start,
            "cron_schedule": cron_schedule
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

    fn session_id(&self, name: &str) -> i64 {
        let db = self.state.db.lock().expect("db lock");
        db.query_row("SELECT id FROM sessions WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .expect("session id")
    }

    fn event_count(&self, name: &str, event: &str, trigger: &str) -> i64 {
        let db = self.state.db.lock().expect("db lock");
        db.query_row(
            "SELECT COUNT(*)
             FROM session_events se
             INNER JOIN sessions s ON s.id = se.session_id
             WHERE s.name = ?1 AND se.event = ?2 AND se.trigger = ?3",
            rusqlite::params![name, event, trigger],
            |r| r.get(0),
        )
        .expect("event count")
    }

    fn status_for(&self, name: &str) -> String {
        let db = self.state.db.lock().expect("db lock");
        db.query_row(
            "SELECT ss.status
             FROM session_status ss
             INNER JOIN sessions s ON s.id = ss.session_id
             WHERE s.name = ?1",
            [name],
            |r| r.get(0),
        )
        .expect("status")
    }
}

impl Drop for E2eHarness {
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
async fn t_e_01_daemon_starts_creates_db_and_serves_health() {
    let h = E2eHarness::new().await;
    let response = h
        .client
        .get(format!("{}/health", h.base_url))
        .send()
        .await
        .expect("health request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("health json");
    assert_eq!(body["status"], "ok");
    assert!(body["db_path"].as_str().is_some());
}

#[tokio::test]
async fn t_e_02_add_session_auto_start_within_single_poll_cycle() {
    let h = E2eHarness::new().await;
    h.create_session("te02-auto", true, None).await;

    let _guard = env_lock().lock().await;
    let tmux_script = write_script("#!/bin/sh\nexit 1\n");
    let tmuxp_script = write_script("#!/bin/sh\nexit 0\n");
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", tmux_script.to_string_lossy().as_ref());
    let prev_tmuxp = set_env_var("SCMUX_TMUXP_BIN", tmuxp_script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&h.state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);
    restore_env_var("SCMUX_TMUXP_BIN", prev_tmuxp);
    poll_result.expect("poll cycle");

    assert_eq!(h.event_count("te02-auto", "started", "auto_start"), 1);
}

#[tokio::test]
async fn t_e_03_kill_session_externally_detected_stopped_on_next_poll() {
    let h = E2eHarness::new().await;
    h.create_session("te03-stop", false, None).await;
    let session_id = h.session_id("te03-stop");
    {
        let db = h.state.db.lock().expect("db lock");
        db.execute(
            "INSERT INTO session_status (session_id, status, polled_at)
             VALUES (?1, 'running', datetime('now'))",
            [session_id],
        )
        .expect("seed running status");
    }

    let _guard = env_lock().lock().await;
    let tmux_script = write_script("#!/bin/sh\nexit 1\n");
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", tmux_script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&h.state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);
    poll_result.expect("poll cycle");

    assert_eq!(h.status_for("te03-stop"), "stopped");
    assert!(h.event_count("te03-stop", "stopped", "daemon") >= 1);
}

#[tokio::test]
async fn t_e_04_post_start_makes_session_running() {
    let h = E2eHarness::new().await;
    h.create_session("te04-start", false, None).await;
    let state_file = h._tmp.path().join("te04-running.flag");

    let _guard = env_lock().lock().await;
    let tmuxp_script = write_script(&format!(
        r#"#!/bin/sh
touch "{}"
exit 0
"#,
        state_file.to_string_lossy()
    ));
    let tmux_script = write_script(&format!(
        r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  if [ -f "{}" ]; then
    echo "te04-start"
    exit 0
  fi
  exit 1
fi
if [ "$1" = "list-panes" ]; then
  echo "0|lead|zsh|1"
  exit 0
fi
exit 1
"#,
        state_file.to_string_lossy()
    ));

    let prev_tmuxp = set_env_var("SCMUX_TMUXP_BIN", tmuxp_script.to_string_lossy().as_ref());
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", tmux_script.to_string_lossy().as_ref());

    let start_response = h
        .client
        .post(format!("{}/sessions/te04-start/start", h.base_url))
        .send()
        .await
        .expect("start request");
    assert_eq!(start_response.status(), reqwest::StatusCode::OK);
    let body: Value = start_response.json().await.expect("json");
    assert_eq!(body["ok"], true);

    scheduler::poll_cycle(&h.state).await.expect("poll cycle");
    restore_env_var("SCMUX_TMUXP_BIN", prev_tmuxp);
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);

    assert_eq!(h.status_for("te04-start"), "running");
}

#[tokio::test]
async fn t_e_05_post_stop_makes_session_disappear() {
    let h = E2eHarness::new().await;
    h.create_session("te05-stop", false, None).await;
    let state_file = h._tmp.path().join("te05-running.flag");
    std::fs::write(&state_file, "running").expect("seed flag");

    let _guard = env_lock().lock().await;
    let tmux_script = write_script(&format!(
        r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  if [ -f "{}" ]; then
    echo "te05-stop"
    exit 0
  fi
  exit 1
fi
if [ "$1" = "list-panes" ]; then
  echo "0|lead|zsh|1"
  exit 0
fi
if [ "$1" = "kill-session" ]; then
  rm -f "{}"
  exit 0
fi
exit 1
"#,
        state_file.to_string_lossy(),
        state_file.to_string_lossy()
    ));
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", tmux_script.to_string_lossy().as_ref());

    scheduler::poll_cycle(&h.state)
        .await
        .expect("poll cycle running");
    let stop_response = h
        .client
        .post(format!("{}/sessions/te05-stop/stop", h.base_url))
        .send()
        .await
        .expect("stop request");
    assert_eq!(stop_response.status(), reqwest::StatusCode::OK);
    scheduler::poll_cycle(&h.state)
        .await
        .expect("poll cycle stopped");
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);

    assert_eq!(h.status_for("te05-stop"), "stopped");
}

#[tokio::test]
async fn t_e_07_cron_session_starts_at_scheduled_time_with_injected_clock() {
    let fixed_now = chrono::DateTime::parse_from_rfc3339("2026-03-07T10:00:10Z")
        .expect("fixed timestamp")
        .with_timezone(&Utc);
    let clock = Arc::new(FixedClock { now: fixed_now });
    let h = E2eHarness::new_with_clock(clock).await;

    let cron = format!(
        "{} {} {} {} {} *",
        fixed_now.second(),
        fixed_now.minute(),
        fixed_now.hour(),
        fixed_now.day(),
        fixed_now.month()
    );
    h.create_session("te07-cron", false, Some(&cron)).await;

    let _guard = env_lock().lock().await;
    let tmux_script = write_script("#!/bin/sh\nexit 1\n");
    let tmuxp_script = write_script("#!/bin/sh\nexit 0\n");
    let prev_tmux = set_env_var("SCMUX_TMUX_BIN", tmux_script.to_string_lossy().as_ref());
    let prev_tmuxp = set_env_var("SCMUX_TMUXP_BIN", tmuxp_script.to_string_lossy().as_ref());

    let poll_result = scheduler::poll_cycle(&h.state).await;
    restore_env_var("SCMUX_TMUX_BIN", prev_tmux);
    restore_env_var("SCMUX_TMUXP_BIN", prev_tmuxp);
    poll_result.expect("poll cycle");

    assert_eq!(h.event_count("te07-cron", "started", "cron"), 1);
}
