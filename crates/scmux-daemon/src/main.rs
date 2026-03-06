mod db;
mod tmux;
mod scheduler;
mod api;

use std::sync::Arc;
use tracing::info;

pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub host_id: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("SCMUX_DB")
        .unwrap_or_else(|_| format!("{}/.config/scmux/scmux.db",
            std::env::var("HOME").unwrap_or_else(|_| ".".into())));

    std::fs::create_dir_all(std::path::Path::new(&db_path).parent().unwrap())?;

    let conn = db::open(&db_path)?;
    let host_id = db::ensure_local_host(&conn)?;

    info!("scmux-daemon starting — db={db_path} host_id={host_id}");

    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        host_id,
    });

    // Poll loop — every 15 seconds
    let poll_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(15)
        );
        loop {
            interval.tick().await;
            if let Err(e) = scheduler::poll_cycle(&poll_state).await {
                tracing::error!("poll cycle error: {e}");
            }
        }
    });

    // Health loop — every 60 seconds
    let health_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(60)
        );
        loop {
            interval.tick().await;
            if let Err(e) = db::write_health(&health_state).await {
                tracing::error!("health write error: {e}");
            }
        }
    });

    // HTTP server
    let port: u16 = std::env::var("SCMUX_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7700);

    let addr = format!("0.0.0.0:{port}");
    info!("HTTP listening on {addr}");

    let router = api::router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
