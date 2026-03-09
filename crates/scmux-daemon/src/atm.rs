use crate::{runtime::AtmRuntimeUpdate, AppState};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const SOCKET_TIMEOUT_SECS: u64 = 2;
const DEFAULT_STUCK_MINUTES: u64 = 10;

#[derive(Debug, Clone)]
pub struct ShutdownTarget {
    pub team: String,
    pub agent: String,
}

#[derive(Debug, Deserialize)]
struct SocketResponse<T> {
    status: String,
    payload: Option<T>,
    #[allow(dead_code)]
    error: Option<SocketError>,
}

#[derive(Debug, Deserialize)]
struct SocketError {
    #[allow(dead_code)]
    code: String,
    #[allow(dead_code)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct AgentEntry {
    agent: String,
    #[serde(default)]
    state: String,
}

#[derive(Debug, Deserialize)]
struct AgentStateEntry {
    #[serde(default)]
    state: String,
    #[serde(default)]
    last_transition: Option<String>,
}

pub async fn poll_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    if !state.config.atm.enabled {
        state.atm_available.store(false, Ordering::Relaxed);
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.clear_atm();
        return Ok(());
    }

    let socket_path = resolve_socket_path(state);
    let teams = configured_teams(state);

    if teams.is_empty() {
        state.atm_available.store(false, Ordering::Relaxed);
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.clear_atm();
        return Ok(());
    }

    let stuck_minutes = state
        .config
        .atm
        .stuck_minutes
        .unwrap_or(DEFAULT_STUCK_MINUTES) as i64;
    let now = state.clock.now_utc();

    let mut updates = Vec::new();
    let mut successful_teams = 0usize;
    let mut first_error: Option<anyhow::Error> = None;

    for team in teams {
        let agents = match list_agents(&socket_path, &team).await {
            Ok(agents) => {
                successful_teams += 1;
                agents
            }
            Err(err) => {
                tracing::warn!("atm list-agents failed for team '{}': {}", team, err);
                if first_error.is_none() {
                    first_error = Some(err);
                }
                continue;
            }
        };

        for agent in agents {
            let state_entry = match query_agent_state(&socket_path, &team, &agent.agent).await {
                Ok(entry) => Some(entry),
                Err(err) => {
                    tracing::warn!(
                        "atm agent-state failed for team='{}' agent='{}': {}",
                        team,
                        agent.agent,
                        err
                    );
                    None
                }
            };

            let base_state = state_entry
                .as_ref()
                .map(|entry| entry.state.as_str())
                .unwrap_or(agent.state.as_str());
            let last_transition = state_entry
                .as_ref()
                .and_then(|entry| entry.last_transition.clone());
            let mut derived_state = normalize_state(base_state).to_string();
            if is_stuck(
                &derived_state,
                last_transition.as_deref(),
                now,
                stuck_minutes,
            ) {
                derived_state = "stuck".to_string();
            }

            updates.push(AtmRuntimeUpdate {
                team: team.clone(),
                agent: agent.agent.clone(),
                state: derived_state,
                last_transition,
            });
        }
    }

    if successful_teams == 0 {
        state.atm_available.store(false, Ordering::Relaxed);
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.clear_atm();
        return Err(first_error.unwrap_or_else(|| anyhow!("ATM query failed for all teams")));
    }

    {
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.apply_atm_updates(updates);
    }

    state.atm_available.store(true, Ordering::Relaxed);
    Ok(())
}

pub async fn send_shutdown_messages(
    state: &AppState,
    targets: &[ShutdownTarget],
) -> anyhow::Result<usize> {
    if !state.config.atm.enabled || !state.config.atm.allow_shutdown {
        return Ok(0);
    }

    if targets.is_empty() {
        return Ok(0);
    }

    let configured_targets = targets
        .iter()
        .map(|target| format!("{}@{}", target.agent, target.team))
        .collect::<Vec<_>>();
    tracing::warn!(
        "ATM send not implemented: skipping graceful shutdown send; targets={:?}",
        configured_targets
    );
    Ok(0)
}

fn resolve_socket_path(state: &AppState) -> PathBuf {
    if let Some(path) = state
        .config
        .atm
        .socket_path
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return PathBuf::from(path);
    }
    atm_home_dir().join(".claude/daemon/atm-daemon.sock")
}

fn atm_home_dir() -> PathBuf {
    std::env::var_os("ATM_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn configured_teams(state: &AppState) -> Vec<String> {
    state
        .config
        .atm
        .teams
        .iter()
        .map(|team| team.trim().to_string())
        .filter(|team| !team.is_empty())
        .collect::<Vec<_>>()
}

fn normalize_state(state: &str) -> &'static str {
    match state.trim().to_ascii_lowercase().as_str() {
        "active" => "active",
        "idle" => "idle",
        "offline" => "offline",
        "unknown" => "unknown",
        "stuck" => "stuck",
        _ => "unknown",
    }
}

fn is_stuck(
    state: &str,
    last_transition: Option<&str>,
    now: DateTime<Utc>,
    stuck_minutes: i64,
) -> bool {
    if state != "active" {
        return false;
    }
    let Some(last_transition) = last_transition else {
        return false;
    };
    let Ok(last_dt) = DateTime::parse_from_rfc3339(last_transition) else {
        return false;
    };
    now.signed_duration_since(last_dt.with_timezone(&Utc))
        > chrono::Duration::minutes(stuck_minutes)
}

async fn list_agents(socket_path: &Path, team: &str) -> anyhow::Result<Vec<AgentEntry>> {
    query_socket(
        socket_path,
        "list-agents",
        serde_json::json!({
            "team": team,
        }),
    )
    .await
}

async fn query_agent_state(
    socket_path: &Path,
    team: &str,
    agent: &str,
) -> anyhow::Result<AgentStateEntry> {
    query_socket(
        socket_path,
        "agent-state",
        serde_json::json!({
            "team": team,
            "agent": agent,
        }),
    )
    .await
}

async fn query_socket<T: DeserializeOwned>(
    socket_path: &Path,
    command: &str,
    payload: serde_json::Value,
) -> anyhow::Result<T> {
    let request = serde_json::json!({
        "version": 1,
        "request_id": request_id(),
        "command": command,
        "payload": payload,
    });

    let response: SocketResponse<T> = query_socket_raw(socket_path, &request.to_string()).await?;
    if response.status != "ok" {
        return Err(anyhow!("ATM daemon returned status '{}'", response.status));
    }
    response
        .payload
        .ok_or_else(|| anyhow!("ATM daemon response missing payload"))
}

fn request_id() -> String {
    format!(
        "scmux-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

#[cfg(unix)]
async fn query_socket_raw<T: DeserializeOwned>(
    socket_path: &Path,
    request_json: &str,
) -> anyhow::Result<T> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let mut stream = tokio::time::timeout(
        tokio::time::Duration::from_secs(SOCKET_TIMEOUT_SECS),
        UnixStream::connect(socket_path),
    )
    .await
    .map_err(|_| anyhow!("ATM socket connect timeout"))??;

    tokio::time::timeout(
        tokio::time::Duration::from_secs(SOCKET_TIMEOUT_SECS),
        stream.write_all(request_json.as_bytes()),
    )
    .await
    .map_err(|_| anyhow!("ATM socket write timeout"))??;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    tokio::time::timeout(
        tokio::time::Duration::from_secs(SOCKET_TIMEOUT_SECS),
        reader.read_line(&mut line),
    )
    .await
    .map_err(|_| anyhow!("ATM socket read timeout"))??;

    if line.trim().is_empty() {
        return Err(anyhow!("ATM socket returned empty response"));
    }

    let parsed = serde_json::from_str::<T>(&line)?;
    Ok(parsed)
}

#[cfg(not(unix))]
async fn query_socket_raw<T: DeserializeOwned>(
    _socket_path: &Path,
    _request_json: &str,
) -> anyhow::Result<T> {
    Err(anyhow!("ATM socket IPC is only available on Unix"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AtmConfig, Config, DaemonConfig, PollingConfig};
    use crate::{ci, db, definition_writer, runtime, AppState, RuntimeHealth, SystemClock};
    use tempfile::TempDir;

    fn build_state(atm: AtmConfig) -> (TempDir, AppState) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("atm-tests.db");
        let conn = db::open(db_path.to_str().expect("utf8 path")).expect("open db");
        let host_id = definition_writer::ensure_local_host(&conn).expect("local host");

        (
            tmp,
            AppState {
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
                    atm,
                },
                reachability: std::sync::Mutex::new(std::collections::HashMap::new()),
                runtime: std::sync::Mutex::new(runtime::RuntimeProjection::default()),
                ci_tools: ci::ToolAvailability::default(),
                clock: std::sync::Arc::new(SystemClock),
                atm_available: std::sync::atomic::AtomicBool::new(false),
                last_api_access: std::sync::atomic::AtomicU64::new(0),
                started_at: std::time::Instant::now(),
                health: std::sync::Mutex::new(RuntimeHealth::default()),
            },
        )
    }

    #[tokio::test]
    async fn td_atm_09_shutdown_send_returns_early_when_allow_shutdown_false() {
        let (_tmp, state) = build_state(AtmConfig {
            enabled: true,
            teams: vec!["scmux-dev".to_string()],
            allow_shutdown: false,
            socket_path: None,
            stuck_minutes: Some(10),
            stop_grace_secs: None,
        });

        let sent = send_shutdown_messages(
            &state,
            &[ShutdownTarget {
                team: "scmux-dev".to_string(),
                agent: "team-lead".to_string(),
            }],
        )
        .await
        .expect("send");

        assert_eq!(sent, 0);
    }

    #[test]
    fn td_atm_teams_are_config_only_not_scanned_from_home() {
        let home = tempfile::tempdir().expect("home");
        std::fs::create_dir_all(home.path().join(".claude/teams/should-not-load"))
            .expect("create teams dir");
        let prev_home = std::env::var("HOME").ok();
        // SAFETY: test-only env mutation within single test scope.
        unsafe { std::env::set_var("HOME", home.path()) };

        let (_tmp, state) = build_state(AtmConfig {
            enabled: true,
            teams: vec!["scmux-dev".to_string()],
            allow_shutdown: false,
            socket_path: None,
            stuck_minutes: Some(10),
            stop_grace_secs: None,
        });

        let teams = configured_teams(&state);
        assert_eq!(teams, vec!["scmux-dev".to_string()]);

        match prev_home {
            Some(value) => {
                // SAFETY: restoring previous test-only env var value.
                unsafe { std::env::set_var("HOME", value) };
            }
            None => {
                // SAFETY: restoring previous test-only env var absence.
                unsafe { std::env::remove_var("HOME") };
            }
        }
    }
}
