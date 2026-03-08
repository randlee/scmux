pub mod api;
pub mod atm;
pub mod ci;
pub mod config;
pub mod db;
pub mod definition_writer;
pub mod hosts;
pub mod logging;
pub mod runtime;
pub mod scheduler;
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
}
