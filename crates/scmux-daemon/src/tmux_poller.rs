use std::sync::Arc;

use crate::{db, runtime::ConfiguredPane, start_cycle, tmux, AppState};

pub use crate::start_cycle::should_run_now;

pub async fn poll_cycle(state: &Arc<AppState>) -> anyhow::Result<()> {
    let live = tmux::live_sessions().await?;

    let sessions = {
        let state = Arc::clone(state);
        tokio::task::spawn_blocking(move || {
            let db = state.db.lock().expect("db lock");
            db::list_sessions_for_host(&db, state.host_id)
        })
        .await??
    };

    let defined_names = sessions
        .iter()
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    let pane_configs = sessions
        .iter()
        .map(|session| {
            (
                session.name.clone(),
                configured_panes(&session.config_json).unwrap_or_default(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let polled_at = state.clock.now_utc().to_rfc3339();

    {
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.apply_tmux_snapshot(&defined_names, &live, &pane_configs, &polled_at);
    }

    start_cycle::run_start_cycle(state, &sessions, &live).await?;
    Ok(())
}

fn configured_panes(config_json: &str) -> Option<Vec<ConfiguredPane>> {
    let value: serde_json::Value = serde_json::from_str(config_json).ok()?;
    let panes = value.get("panes")?.as_array()?;
    Some(
        panes
            .iter()
            .map(|pane| ConfiguredPane {
                name: pane
                    .get("name")
                    .and_then(|raw| raw.as_str())
                    .map(ToOwned::to_owned),
                command: pane
                    .get("command")
                    .and_then(|raw| raw.as_str())
                    .map(ToOwned::to_owned),
                atm_team: pane
                    .get("atm_team")
                    .and_then(|raw| raw.as_str())
                    .map(ToOwned::to_owned),
                atm_agent: pane
                    .get("atm_agent")
                    .and_then(|raw| raw.as_str())
                    .map(ToOwned::to_owned),
            })
            .collect(),
    )
}
