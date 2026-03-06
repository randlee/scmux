use clap::Parser;
use scmux_daemon::config::Config;
use scmux_daemon::{api, db, hosts, logging, scheduler, AppState};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

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
    let home_dir = home_dir();
    let config = Config::load()?;
    // Determine effective log level: --verbose > config > SCMUX_LOG env var > default
    let effective_level = if args.verbose {
        "debug".to_string()
    } else if let Some(ref level) = config.daemon.log_level {
        level.clone()
    } else {
        std::env::var("SCMUX_LOG").unwrap_or_else(|_| "info".to_string())
    };
    // SAFETY: single-threaded startup, no tokio workers yet.
    unsafe { std::env::set_var("SCMUX_LOG", &effective_level) };

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

    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

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
        reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
        last_api_access: std::sync::atomic::AtomicU64::new(0),
        started_at: std::time::Instant::now(),
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

    // Host reachability loop — adaptive interval based on API activity and live sessions
    let host_poll_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            if let Err(e) = hosts::poll_hosts(Arc::clone(&host_poll_state)).await {
                tracing::warn!("host poll error: {e}");
            }

            let active = hosts::should_use_active_interval(&host_poll_state)
                .await
                .unwrap_or(false);
            let sleep_secs = if active {
                health_interval_secs
            } else {
                health_interval_secs.saturating_mul(10)
            }
            .max(1);
            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)).await;
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
