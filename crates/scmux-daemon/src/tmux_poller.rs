use std::sync::Arc;

use crate::{db, scheduler, tmux, AppState};

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
    let polled_at = state.clock.now_utc().to_rfc3339();

    {
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.apply_tmux_snapshot(&defined_names, &live, &polled_at);
    }

    scheduler::run_start_cycle(state, &sessions, &live).await?;
    Ok(())
}
