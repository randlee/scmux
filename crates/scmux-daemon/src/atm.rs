use crate::{db, AppState};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::warn;

const SOCKET_TIMEOUT_SECS: u64 = 2;
const DEFAULT_STUCK_MINUTES: u64 = 10;

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
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentStateEntry {
    #[serde(default)]
    state: String,
    #[serde(default)]
    last_transition: Option<String>,
}

pub async fn poll_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let socket_path = resolve_socket_path(state);
    let teams = discover_teams();

    if teams.is_empty() {
        state.atm_available.store(false, Ordering::Relaxed);
        return Ok(());
    }

    let stuck_minutes = state
        .config
        .atm
        .stuck_minutes
        .unwrap_or(DEFAULT_STUCK_MINUTES) as i64;
    let now = state.clock.now_utc();
    let updated_at = now.to_rfc3339();
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
                warn!("atm list-agents failed for team '{team}': {err}");
                if first_error.is_none() {
                    first_error = Some(err);
                }
                continue;
            }
        };

        for agent in agents {
            let Some(session_name) = extract_session_name(agent.session_id.as_deref()) else {
                continue;
            };

            let state_entry = match query_agent_state(&socket_path, &team, &agent.agent).await {
                Ok(entry) => Some(entry),
                Err(err) => {
                    warn!(
                        "atm agent-state failed for team='{}' agent='{}': {}",
                        team, agent.agent, err
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

            updates.push(db::SessionAtmUpdate {
                session_name,
                agent_id: agent.agent.clone(),
                team: team.clone(),
                state: derived_state,
                last_transition,
                updated_at: updated_at.clone(),
            });
        }
    }

    if successful_teams == 0 {
        state.atm_available.store(false, Ordering::Relaxed);
        return Err(first_error.unwrap_or_else(|| anyhow!("ATM query failed for all teams")));
    }

    {
        let state = Arc::clone(state);
        let updates_for_db = updates;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let db_conn = state.db.lock().expect("db lock");
            db::replace_session_atm(&db_conn, &updates_for_db)
        })
        .await??;
    }

    state.atm_available.store(true, Ordering::Relaxed);
    Ok(())
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
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from)) // QA-046+P5S3: macOS-only daemon, HOME always set
        .unwrap_or_else(|| PathBuf::from("."))
}

fn discover_teams() -> Vec<String> {
    let teams_dir = atm_home_dir().join(".claude/teams");
    let Ok(entries) = std::fs::read_dir(teams_dir) else {
        return Vec::new();
    };

    let mut teams = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join("config.json").exists() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            teams.push(name.to_string());
        }
    }
    teams.sort();
    teams
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

fn extract_session_name(session_id: Option<&str>) -> Option<String> {
    let value = session_id?.trim();
    if value.is_empty() {
        return None;
    }
    let session = value.split(':').next().unwrap_or_default().trim();
    if session.is_empty() {
        None
    } else {
        Some(session.to_string())
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
