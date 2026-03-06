pub mod api;
pub mod config;
pub mod db;
pub mod logging;
pub mod scheduler;
pub mod tmux;

#[derive(Debug)]
pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub host_id: i64,
    pub config: config::Config,
}
