use chrono::Utc;
use cron::Schedule;
use rusqlite::params;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

use crate::{tmux, AppState};

struct SessionRow {
    id: i64,
    name: String,
    cron_schedule: Option<String>,
    auto_start: bool,
    config_json: String,
}

pub async fn poll_cycle(state: &Arc<AppState>) -> anyhow::Result<()> {
    let live = tmux::live_sessions().await?;

    // Phase 1: Read sessions via spawn_blocking (rusqlite::Connection is !Send)
    let mut sessions = load_enabled_sessions_for_host(state).await?;

    // NF-06: If the DB was lost while daemon was down, rebuild local sessions from live tmux.
    if sessions.is_empty() && !live.is_empty() {
        match reconstruct_registry_from_live(state, &live).await {
            Ok(0) => {}
            Ok(recovered) => {
                info!("reconstructed {recovered} local sessions from live tmux");
                sessions = load_enabled_sessions_for_host(state).await?;
            }
            Err(err) => warn!("failed to reconstruct session registry from tmux: {err}"),
        }
    }

    let now = Utc::now();

    // Phase 2: Update status and write transition events via spawn_blocking
    let state2 = Arc::clone(state);
    let live2 = live.clone();
    let sessions2 = sessions
        .iter()
        .map(|s| (s.id, s.name.clone()))
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        let db = state2.db.lock().unwrap();
        for (id, name) in &sessions2 {
            let is_live = live2.contains_key(name);
            let status = if is_live { "running" } else { "stopped" };
            let panes_json = live2
                .get(name)
                .map(|p| serde_json::to_string(p).unwrap_or_default());
            let previous_status: Option<String> = db
                .query_row(
                    "SELECT status FROM session_status WHERE session_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .ok();

            if let Err(err) = db.execute(
                "INSERT INTO session_status (session_id, status, panes_json, polled_at)
                 VALUES (?1, ?2, ?3, datetime('now'))
                 ON CONFLICT(session_id) DO UPDATE SET
                   status     = excluded.status,
                   panes_json = excluded.panes_json,
                   polled_at  = excluded.polled_at",
                params![id, status, panes_json],
            ) {
                warn!("status upsert failed for session '{name}': {err}");
                continue;
            }

            if let Some(prev) = previous_status.as_deref() {
                if prev != status {
                    let event = if is_live { "started" } else { "stopped" };
                    if let Err(err) = db.execute(
                        "INSERT INTO session_events (session_id, event, trigger)
                         VALUES (?1, ?2, 'daemon')",
                        params![id, event],
                    ) {
                        warn!("event write failed for session '{name}': {err}");
                    }
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;

    // Phase 3: Determine what to start (no DB access, no await)
    let mut to_start: Vec<(i64, String, String, String)> = Vec::new();
    for session in &sessions {
        if live.contains_key(&session.name) {
            continue;
        }
        if session.auto_start {
            to_start.push((
                session.id,
                session.name.clone(),
                session.config_json.clone(),
                "auto_start".into(),
            ));
            continue;
        }
        if let Some(ref expr) = session.cron_schedule {
            if should_run_now(expr, &now) {
                to_start.push((
                    session.id,
                    session.name.clone(),
                    session.config_json.clone(),
                    "cron".into(),
                ));
            }
        }
    }

    // Phase 4: Start sessions — each DB write in its own spawn_blocking
    for (id, name, config_json, trigger) in to_start {
        info!("starting session '{name}' trigger={trigger}");
        match tmux::start_session(&name, &config_json).await {
            Ok(()) => {
                let state4 = Arc::clone(state);
                let trigger4 = trigger.clone();
                let write_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let db = state4.db.lock().unwrap();
                    db.execute(
                        "INSERT INTO session_events (session_id, event, trigger)
                         VALUES (?1, 'started', ?2)",
                        params![id, trigger4],
                    )?;
                    Ok(())
                })
                .await;
                match write_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => warn!("failed to log start event for '{name}': {err}"),
                    Err(err) => warn!("failed to join start-event task for '{name}': {err}"),
                }
                info!("session '{name}' started");
            }
            Err(e) => {
                warn!("failed to start session '{name}': {e}");
                let state4 = Arc::clone(state);
                let trigger4 = trigger.clone();
                let note = e.to_string();
                let write_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let db = state4.db.lock().unwrap();
                    db.execute(
                        "INSERT INTO session_events (session_id, event, trigger, note)
                         VALUES (?1, 'failed', ?2, ?3)",
                        params![id, trigger4, note],
                    )?;
                    Ok(())
                })
                .await;
                match write_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => warn!("failed to log failure event for '{name}': {err}"),
                    Err(err) => warn!("failed to join failure-event task for '{name}': {err}"),
                }
            }
        }
    }

    Ok(())
}

async fn load_enabled_sessions_for_host(state: &Arc<AppState>) -> anyhow::Result<Vec<SessionRow>> {
    let state1 = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let db = state1.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name, cron_schedule, auto_start, config_json
             FROM sessions
             WHERE host_id = ?1 AND enabled = 1",
        )?;
        let rows: Vec<SessionRow> = stmt
            .query_map(params![state1.host_id], |r| {
                Ok(SessionRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    cron_schedule: r.get(2)?,
                    auto_start: r.get(3)?,
                    config_json: r.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok::<_, anyhow::Error>(rows)
    })
    .await?
}

async fn reconstruct_registry_from_live(
    state: &Arc<AppState>,
    live: &std::collections::HashMap<String, Vec<tmux::PaneInfo>>,
) -> anyhow::Result<usize> {
    let state = Arc::clone(state);
    let live = live.clone();
    tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        let session_count: i64 = db
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap_or(0);
        if session_count > 0 {
            return Ok::<_, anyhow::Error>(0);
        }

        for name in live.keys() {
            let config_json = serde_json::json!({ "session_name": name }).to_string();
            db.execute(
                "INSERT INTO sessions (
                    name, project, host_id, config_json, cron_schedule, auto_start, enabled
                 ) VALUES (?1, NULL, ?2, ?3, NULL, 0, 1)
                 ON CONFLICT(name, host_id) DO UPDATE SET
                    enabled = 1,
                    config_json = excluded.config_json",
                params![name, state.host_id, config_json],
            )?;
        }

        for (name, panes) in &live {
            let session_id: i64 = db.query_row(
                "SELECT id FROM sessions WHERE host_id = ?1 AND name = ?2",
                params![state.host_id, name],
                |r| r.get(0),
            )?;
            let panes_json = serde_json::to_string(panes).unwrap_or_else(|_| "[]".to_string());
            db.execute(
                "INSERT INTO session_status (session_id, status, panes_json, polled_at)
                 VALUES (?1, 'running', ?2, datetime('now'))
                 ON CONFLICT(session_id) DO UPDATE SET
                    status = excluded.status,
                    panes_json = excluded.panes_json,
                    polled_at = excluded.polled_at",
                params![session_id, panes_json],
            )?;
        }

        Ok::<_, anyhow::Error>(live.len())
    })
    .await?
}

/// Returns true if the cron expression should fire within the current 15s window.
pub fn should_run_now(expr: &str, now: &chrono::DateTime<Utc>) -> bool {
    let normalized = if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    };
    let Ok(schedule) = Schedule::from_str(&normalized) else {
        return false;
    };
    let window_start = *now - chrono::Duration::seconds(15);
    schedule
        .after(&window_start)
        .next()
        .map(|t| t <= *now)
        .unwrap_or(false)
}
