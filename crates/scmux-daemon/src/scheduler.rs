use std::sync::Arc;
use rusqlite::params;
use tracing::{info, warn};
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;

use crate::AppState;
use crate::tmux;

pub async fn poll_cycle(state: &Arc<AppState>) -> anyhow::Result<()> {
    let live = tmux::live_sessions().await?;
    let db = state.db.lock().await;

    let mut stmt = db.prepare(
        "SELECT id, name, cron_schedule, auto_start, config_json
         FROM sessions
         WHERE host_id = ?1 AND enabled = 1"
    )?;

    struct SessionRow {
        id: i64,
        name: String,
        cron_schedule: Option<String>,
        auto_start: bool,
        config_json: String,
    }

    let sessions: Vec<SessionRow> = stmt.query_map(params![state.host_id], |r| {
        Ok(SessionRow {
            id: r.get(0)?,
            name: r.get(1)?,
            cron_schedule: r.get(2)?,
            auto_start: r.get(3)?,
            config_json: r.get(4)?,
        })
    })?.filter_map(|r| r.ok()).collect();

    let now = Utc::now();

    for session in &sessions {
        let is_live = live.contains_key(&session.name);
        let status = if is_live { "running" } else { "stopped" };
        let panes_json = live.get(&session.name)
            .map(|p| serde_json::to_string(p).unwrap_or_default());

        // Upsert session_status
        db.execute(
            "INSERT INTO session_status (session_id, status, panes_json, polled_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(session_id) DO UPDATE SET
               status     = excluded.status,
               panes_json = excluded.panes_json,
               polled_at  = excluded.polled_at",
            params![session.id, status, panes_json],
        )?;

        // Record stop event if session just disappeared
        if !is_live {
            let was_running: bool = db.query_row(
                "SELECT COUNT(*) > 0 FROM session_events
                 WHERE session_id = ?1 AND event = 'started'
                 AND occurred_at > (
                   SELECT COALESCE(MAX(occurred_at), '1970-01-01')
                   FROM session_events
                   WHERE session_id = ?1 AND event = 'stopped'
                 )",
                params![session.id],
                |r| r.get(0),
            ).unwrap_or(false);

            if was_running {
                db.execute(
                    "INSERT INTO session_events (session_id, event, trigger) VALUES (?1, 'stopped', 'daemon')",
                    params![session.id],
                )?;
            }
        }
    }

    drop(db);

    // Collect candidates for start
    let mut to_start: Vec<(i64, String, String, String)> = Vec::new();

    {
        let db = state.db.lock().await;
        for session in &sessions {
            let is_live = live.contains_key(&session.name);
            if is_live { continue; }

            if session.auto_start {
                to_start.push((session.id, session.name.clone(), session.config_json.clone(), "auto_start".into()));
                continue;
            }

            if let Some(ref expr) = session.cron_schedule {
                if should_run_now(expr, &now) {
                    to_start.push((session.id, session.name.clone(), session.config_json.clone(), "cron".into()));
                }
            }
        }
    }

    for (id, name, config_json, trigger) in to_start {
        info!("starting session '{name}' trigger={trigger}");
        match tmux::start_session(&name, &config_json).await {
            Ok(()) => {
                let db = state.db.lock().await;
                db.execute(
                    "INSERT INTO session_events (session_id, event, trigger) VALUES (?1, 'started', ?2)",
                    params![id, trigger],
                )?;
                info!("session '{name}' started");
            }
            Err(e) => {
                warn!("failed to start session '{name}': {e}");
                let db = state.db.lock().await;
                db.execute(
                    "INSERT INTO session_events (session_id, event, trigger, note) VALUES (?1, 'failed', ?2, ?3)",
                    params![id, trigger, e.to_string()],
                )?;
            }
        }
    }

    Ok(())
}

/// Returns true if the cron expression should fire within the current 15s window.
fn should_run_now(expr: &str, now: &chrono::DateTime<Utc>) -> bool {
    let Ok(schedule) = Schedule::from_str(expr) else { return false; };
    let window_start = *now - chrono::Duration::seconds(15);
    schedule
        .after(&window_start)
        .next()
        .map(|t| t <= *now)
        .unwrap_or(false)
}
