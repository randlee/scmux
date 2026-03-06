mod api;
mod config;
mod db;
mod logging;
mod scheduler;
mod tmux;

use clap::Parser;
use config::Config;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub host_id: i64,
    pub config: Config,
}

const DEFAULT_POLL_INTERVAL_SECS: u64 = 15;
const DEFAULT_HEALTH_INTERVAL_SECS: u64 = 60;
const DEFAULT_PORT: u16 = 7700;

#[derive(Debug, Parser)]
#[command(name = "scmux-daemon")]
struct Args {
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.verbose {
        // SAFETY: called at startup before the tokio runtime spawns any threads.
        unsafe { std::env::set_var("SCMUX_LOG", "debug") };
    }

    let home_dir = home_dir();
    let config_path = std::env::var("SCMUX_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir.join(".config/scmux/scmux.toml"));
    let config = config::load_config(&config_path)?;

    if let Some(log_level) = config.daemon.log_level.as_deref() {
        // SAFETY: called at startup before the tokio runtime spawns any threads.
        unsafe { std::env::set_var("SCMUX_LOG", log_level) };
    }

    let log_path = home_dir.join(".config/scmux/scmux-daemon.log");
    let _log_guards = logging::init_logging(
        "scmux-daemon",
        logging::UnifiedLogMode::DaemonWriter {
            file_path: log_path,
            rotation: logging::RotationConfig::default(),
        },
    )
    .unwrap_or_else(|_| logging::init_stderr_only());

    let db_path = config
        .daemon
        .db_path
        .clone()
        .or_else(|| std::env::var("SCMUX_DB").ok())
        .unwrap_or_else(|| {
            home_dir
                .join(".config/scmux/scmux.db")
                .to_string_lossy()
                .to_string()
        });

    std::fs::create_dir_all(std::path::Path::new(&db_path).parent().unwrap())?;

    let conn = db::open(&db_path)?;
    db::seed_hosts_from_config(
        &conn,
        &config
            .hosts
            .iter()
            .filter(|host| !host.is_local.unwrap_or(false))
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    let host_id = db::ensure_local_host(&conn)?;

    info!("scmux-daemon starting — db={db_path} host_id={host_id}");

    let poll_interval_secs = config
        .polling
        .tmux_interval_secs
        .unwrap_or(DEFAULT_POLL_INTERVAL_SECS);
    let health_interval_secs = config
        .polling
        .health_interval_secs
        .unwrap_or(DEFAULT_HEALTH_INTERVAL_SECS);
    let port: u16 = config
        .daemon
        .port
        .or_else(|| {
            std::env::var("SCMUX_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
        })
        .unwrap_or(DEFAULT_PORT);

    let state = Arc::new(AppState {
        db: std::sync::Mutex::new(conn),
        host_id,
        config,
    });

    // Poll loop — every 15 seconds
    let poll_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(poll_interval_secs));
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
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(health_interval_secs));
        loop {
            interval.tick().await;
            if let Err(e) = db::write_health(&health_state).await {
                tracing::error!("health write error: {e}");
            }
        }
    });

    // HTTP server
    let addr = format!("0.0.0.0:{port}");
    info!("HTTP listening on {addr}");

    let router = api::router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
