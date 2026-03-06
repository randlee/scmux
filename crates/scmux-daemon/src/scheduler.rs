use chrono::Utc;
use cron::Schedule;
use rusqlite::params;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

use crate::tmux;
use crate::AppState;

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
    let state1 = Arc::clone(state);
    let sessions: Vec<SessionRow> = tokio::task::spawn_blocking(move || {
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
    .await??;

    let now = Utc::now();

    // Phase 2: Update status + stop events via spawn_blocking
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

            db.execute(
                "INSERT INTO session_status (session_id, status, panes_json, polled_at)
                 VALUES (?1, ?2, ?3, datetime('now'))
                 ON CONFLICT(session_id) DO UPDATE SET
                   status     = excluded.status,
                   panes_json = excluded.panes_json,
                   polled_at  = excluded.polled_at",
                params![id, status, panes_json],
            )?;

            if !is_live {
                let was_running: bool = db
                    .query_row(
                        "SELECT COUNT(*) > 0 FROM session_events
                         WHERE session_id = ?1 AND event = 'started'
                         AND occurred_at > (
                           SELECT COALESCE(MAX(occurred_at), '1970-01-01')
                           FROM session_events
                           WHERE session_id = ?1 AND event = 'stopped'
                         )",
                        params![id],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);

                if was_running {
                    db.execute(
                        "INSERT INTO session_events (session_id, event, trigger)
                         VALUES (?1, 'stopped', 'daemon')",
                        params![id],
                    )?;
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
                tokio::task::spawn_blocking(move || {
                    let db = state4.db.lock().unwrap();
                    db.execute(
                        "INSERT INTO session_events (session_id, event, trigger)
                         VALUES (?1, 'started', ?2)",
                        params![id, trigger4],
                    )
                })
                .await??;
                info!("session '{name}' started");
            }
            Err(e) => {
                warn!("failed to start session '{name}': {e}");
                let state4 = Arc::clone(state);
                let trigger4 = trigger.clone();
                let note = e.to_string();
                tokio::task::spawn_blocking(move || {
                    let db = state4.db.lock().unwrap();
                    db.execute(
                        "INSERT INTO session_events (session_id, event, trigger, note)
                         VALUES (?1, 'failed', ?2, ?3)",
                        params![id, trigger4, note],
                    )
                })
                .await??;
            }
        }
    }

    Ok(())
}

/// Returns true if the cron expression should fire within the current 15s window.
pub(crate) fn should_run_now(expr: &str, now: &chrono::DateTime<Utc>) -> bool {
    let Ok(schedule) = Schedule::from_str(expr) else {
        return false;
    };
    let window_start = *now - chrono::Duration::seconds(15);
    schedule
        .after(&window_start)
        .next()
        .map(|t| t <= *now)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::should_run_now;
    use chrono::{TimeZone, Utc};

    #[test]
    fn td_05_should_run_now_true_when_cron_fires_in_window() {
        let now = Utc
            .with_ymd_and_hms(2026, 1, 1, 12, 0, 10)
            .single()
            .expect("valid datetime");
        assert!(should_run_now("0 0 12 1 1 *", &now));
    }

    #[test]
    fn td_06_should_run_now_false_when_cron_does_not_fire_in_window() {
        let now = Utc
            .with_ymd_and_hms(2026, 1, 1, 12, 0, 10)
            .single()
            .expect("valid datetime");
        assert!(!should_run_now("0 1 12 1 1 *", &now));
    }

    #[test]
    fn td_07_should_run_now_invalid_cron_returns_false() {
        let now = Utc::now();
        assert!(!should_run_now("not-a-cron", &now));
    }
}
