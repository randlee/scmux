use chrono::Utc;
use cron::Schedule;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

use crate::{db::SessionDefinition, tmux, AppState};

pub async fn poll_cycle(state: &Arc<AppState>) -> anyhow::Result<()> {
    crate::tmux_poller::poll_cycle(state).await
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

pub async fn run_start_cycle(
    state: &Arc<AppState>,
    sessions: &[SessionDefinition],
    live: &HashMap<String, Vec<tmux::PaneInfo>>,
) -> anyhow::Result<()> {
    let now = state.clock.now_utc();
    let mut to_start: Vec<(String, String, String)> = Vec::new();

    for session in sessions {
        if !session.enabled || live.contains_key(&session.name) {
            continue;
        }

        if session.auto_start {
            to_start.push((
                session.name.clone(),
                session.config_json.clone(),
                "auto_start".to_string(),
            ));
            continue;
        }

        if let Some(expr) = session.cron_schedule.as_deref() {
            if should_run_now(expr, &now) {
                to_start.push((
                    session.name.clone(),
                    session.config_json.clone(),
                    "cron".to_string(),
                ));
            }
        }
    }

    for (name, config_json, trigger) in to_start {
        {
            let mut runtime = state.runtime.lock().expect("runtime lock");
            runtime.mark_starting(&name);
        }

        info!("starting session '{}' trigger={}", name, trigger);
        match tmux::start_session(&name, &config_json).await {
            Ok(()) => {
                info!("session '{}' start submitted", name);
            }
            Err(err) => {
                warn!("failed to start session '{}': {}", name, err);
                let mut runtime = state.runtime.lock().expect("runtime lock");
                runtime.mark_start_failed(&name, err.to_string());
            }
        }
    }

    Ok(())
}
