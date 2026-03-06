use crate::{db, AppState};
use anyhow::Context;
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, warn};

const ACTIVE_API_WINDOW_MS: u64 = 60_000;
const REMOTE_FETCH_TIMEOUT_SECS: u64 = 5;

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
    api_port: u16,
    is_local: bool,
    last_seen: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteSessionResponse {
    name: String,
    project: Option<String>,
    #[serde(default)]
    cron_schedule: Option<String>,
    #[serde(default)]
    auto_start: bool,
    #[serde(default = "default_status")]
    status: String,
    #[serde(default = "default_panes")]
    panes: serde_json::Value,
    #[serde(default)]
    polled_at: Option<String>,
}

fn default_status() -> String {
    "stopped".to_string()
}

fn default_panes() -> serde_json::Value {
    serde_json::json!([])
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

pub async fn fetch_remote_sessions(
    address: &str,
    port: u16,
) -> anyhow::Result<Vec<db::RemoteSessionUpsert>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REMOTE_FETCH_TIMEOUT_SECS))
        .build()
        .context("build reqwest client")?;
    let url = format!("http://{address}:{port}/sessions");
    let rows = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("non-success status from {url}"))?
        .json::<Vec<RemoteSessionResponse>>()
        .await
        .with_context(|| format!("decode JSON from {url}"))?;

    Ok(rows
        .into_iter()
        .map(|row| db::RemoteSessionUpsert {
            name: row.name,
            project: row.project,
            cron_schedule: row.cron_schedule,
            auto_start: row.auto_start,
            status: row.status,
            panes_json: serde_json::to_string(&row.panes).unwrap_or_else(|_| "[]".to_string()),
            polled_at: row.polled_at,
        })
        .collect())
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
            let probe_ok = probe_host(&host.address).await;
            let updated = apply_probe_result(previous, probe_ok);

            if probe_ok {
                if let Some(last_seen) = updated.last_seen.clone() {
                    let state2 = Arc::clone(&state);
                    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                        let db_conn = state2.db.lock().expect("db lock");
                        db::update_host_last_seen(&db_conn, host.id, &last_seen)?;
                        Ok(())
                    })
                    .await??;
                }

                match fetch_remote_sessions(&host.address, host.api_port).await {
                    Ok(remote_sessions) => {
                        let state2 = Arc::clone(&state);
                        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                            let db_conn = state2.db.lock().expect("db lock");
                            for session in &remote_sessions {
                                db::upsert_remote_session(&db_conn, host.id, session)?;
                            }
                            Ok(())
                        })
                        .await??;
                    }
                    Err(err) => {
                        warn!(
                            "remote session fetch failed host={} address={}: {}",
                            host.name, host.address, err
                        );
                    }
                }
            }
            updated
        };

        debug!(
            "host poll id={} name={} reachable={} failures={}",
            updated.host_id, host.name, updated.reachable, updated.consecutive_failures
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

    let state2 = Arc::clone(state);
    let running: i64 = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        let db_conn = state2.db.lock().expect("db lock");
        let value = db_conn.query_row(
            "SELECT COUNT(*) FROM session_status WHERE status = 'running'",
            [],
            |r| r.get(0),
        )?;
        Ok(value)
    })
    .await??;
    Ok(running > 0)
}

async fn load_hosts(state: Arc<AppState>) -> anyhow::Result<Vec<HostRow>> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<HostRow>> {
        let db_conn = state.db.lock().expect("db lock");
        let mut stmt = db_conn.prepare(
            "SELECT id, name, address, api_port, is_local, last_seen
             FROM hosts
             ORDER BY is_local DESC, name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(HostRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    address: r.get(2)?,
                    api_port: r.get(3)?,
                    is_local: r.get(4)?,
                    last_seen: r.get(5)?,
                })
            })?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        Ok(rows)
    })
    .await?
}

fn ping_bin() -> String {
    std::env::var("SCMUX_PING_BIN").unwrap_or_else(|_| "ping".to_string())
}
