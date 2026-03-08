use crate::{db, AppState};
use chrono::Utc;
use std::sync::Arc;
use tokio::process::Command;

const ACTIVE_API_WINDOW_MS: u64 = 60_000;

#[derive(Debug, Clone, Default)]
pub struct HostReachability {
    pub host_id: i64,
    pub reachable: bool,
    pub last_seen: Option<String>,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone)]
struct HostRow {
    id: i64,
    name: String,
    address: String,
    is_local: bool,
    last_seen: Option<String>,
}

pub async fn probe_host(address: &str) -> bool {
    let mut command = Command::new(ping_bin());
    if cfg!(target_os = "macos") {
        command.args(["-c", "1", "-t", "10", address]);
    } else {
        command.args(["-c", "1", "-W", "10", address]);
    }
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn apply_probe_result(mut previous: HostReachability, probe_ok: bool) -> HostReachability {
    if probe_ok {
        previous.reachable = true;
        previous.consecutive_failures = 0;
        previous.last_seen = Some(Utc::now().to_rfc3339());
        return previous;
    }

    previous.consecutive_failures = previous.consecutive_failures.saturating_add(1);
    if previous.reachable && previous.consecutive_failures >= 3 {
        previous.reachable = false;
    }
    previous
}

pub async fn poll_hosts(state: Arc<AppState>) -> anyhow::Result<()> {
    let hosts = load_hosts(Arc::clone(&state)).await?;
    let mut next = {
        let map = state.reachability.lock().expect("reachability lock");
        map.clone()
    };

    for host in hosts {
        let previous = next.get(&host.id).cloned().unwrap_or(HostReachability {
            host_id: host.id,
            reachable: host.is_local,
            last_seen: host.last_seen.clone(),
            consecutive_failures: 0,
        });

        let updated = if host.is_local {
            HostReachability {
                host_id: host.id,
                reachable: true,
                last_seen: previous.last_seen,
                consecutive_failures: 0,
            }
        } else {
            apply_probe_result(previous, probe_host(&host.address).await)
        };

        tracing::debug!(
            "host poll id={} name={} reachable={} failures={}",
            updated.host_id,
            host.name,
            updated.reachable,
            updated.consecutive_failures
        );
        next.insert(host.id, updated);
    }

    let mut map = state.reachability.lock().expect("reachability lock");
    *map = next;
    Ok(())
}

pub async fn should_use_active_interval(state: &Arc<AppState>) -> anyhow::Result<bool> {
    let now_ms = state.monotonic_millis();
    let last_access = state
        .last_api_access
        .load(std::sync::atomic::Ordering::Relaxed);
    let api_recent = last_access > 0 && now_ms.saturating_sub(last_access) <= ACTIVE_API_WINDOW_MS;
    if !api_recent {
        return Ok(false);
    }

    let running = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.has_live_sessions()
    };
    Ok(running)
}

async fn load_hosts(state: Arc<AppState>) -> anyhow::Result<Vec<HostRow>> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<HostRow>> {
        let db_conn = state.db.lock().expect("db lock");
        let rows = db::list_hosts(&db_conn)?;
        Ok(rows
            .into_iter()
            .map(|row| HostRow {
                id: row.id,
                name: row.name,
                address: row.address,
                is_local: row.is_local,
                last_seen: row.last_seen,
            })
            .collect::<Vec<_>>())
    })
    .await?
}

fn ping_bin() -> String {
    std::env::var("SCMUX_PING_BIN").unwrap_or_else(|_| "ping".to_string())
}
