pub mod api;
pub mod atm;
pub mod ci;
pub mod config;
pub mod db;
pub mod definition_writer;
pub mod hosts;
pub mod logging;
pub mod runtime;
mod start_cycle;
pub mod tmux;
pub mod tmux_poller;

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc>;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PollerHealth {
    pub status: String,
    pub last_ok: Option<String>,
    pub last_error: Option<String>,
}

impl Default for PollerHealth {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            last_ok: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RuntimeHealth {
    pub tmux: PollerHealth,
    pub hosts: PollerHealth,
    pub ci: PollerHealth,
    pub atm: PollerHealth,
    pub recent_errors: Vec<String>,
}

pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub db_path: String,
    pub host_id: i64,
    pub config: config::Config,
    pub reachability: std::sync::Mutex<std::collections::HashMap<i64, hosts::HostReachability>>,
    pub runtime: std::sync::Mutex<runtime::RuntimeProjection>,
    pub ci_tools: ci::ToolAvailability,
    pub clock: std::sync::Arc<dyn Clock>,
    pub atm_available: std::sync::atomic::AtomicBool,
    pub last_api_access: std::sync::atomic::AtomicU64,
    pub started_at: std::time::Instant,
    pub health: std::sync::Mutex<RuntimeHealth>,
}

impl AppState {
    pub fn monotonic_millis(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    pub fn mark_api_access(&self) {
        self.last_api_access.store(
            self.monotonic_millis(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn mark_poller_ok(&self, poller: &str) {
        let mut health = self.health.lock().expect("health lock");
        let now = chrono::Utc::now().to_rfc3339();
        let target = select_poller_mut(&mut health, poller);
        target.status = "ok".to_string();
        target.last_ok = Some(now);
    }

    pub fn mark_poller_error(&self, poller: &str, error: impl Into<String>) {
        let message = error.into();
        let mut health = self.health.lock().expect("health lock");
        let now = chrono::Utc::now().to_rfc3339();
        let target = select_poller_mut(&mut health, poller);
        target.status = "error".to_string();
        target.last_error = Some(message.clone());
        health
            .recent_errors
            .push(format!("{now} {poller}: {message}"));
        if health.recent_errors.len() > 20 {
            let extra = health.recent_errors.len() - 20;
            health.recent_errors.drain(0..extra);
        }
    }

    pub fn runtime_health(&self) -> RuntimeHealth {
        self.health.lock().expect("health lock").clone()
    }
}

fn select_poller_mut<'a>(health: &'a mut RuntimeHealth, poller: &str) -> &'a mut PollerHealth {
    match poller {
        "tmux" => &mut health.tmux,
        "hosts" => &mut health.hosts,
        "ci" => &mut health.ci,
        "atm" => &mut health.atm,
        _ => &mut health.tmux,
    }
}
