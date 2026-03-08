//! Terminal output formatting helpers for `scmux` commands.

use crate::client::{ActionResponse, HealthResponse, HostSummary, SessionSummary};
use std::collections::HashMap;

pub fn print_session_list(sessions: &[SessionSummary], hosts: &[HostSummary]) {
    let host_names: HashMap<i64, String> = hosts
        .iter()
        .map(|host| (host.id, host.name.clone()))
        .collect();

    println!(
        "{:<15} {:<9} {:<10} {:<10} {:<11} WINDOW",
        "NAME", "STATUS", "ACTIVITY", "HOST", "CRON/AUTO"
    );

    for session in sessions {
        let host = host_names
            .get(&session.host_id)
            .map(|s| s.as_str())
            .unwrap_or("—");
        let cron_auto = session
            .cron_schedule
            .as_deref()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                // If both cron_schedule and auto_start exist, cron takes precedence in display.
                if session.auto_start {
                    "auto".to_string()
                } else {
                    "—".to_string()
                }
            });
        let window = window_name(session);
        let activity = session
            .atm
            .as_ref()
            .map(|atm| atm.state.as_str())
            .unwrap_or("");

        println!(
            "{:<15} {:<9} {:<10} {:<10} {:<11} {}",
            session.name, session.status, activity, host, cron_auto, window
        );
    }
}

pub fn print_hosts(hosts: &[HostSummary]) {
    println!(
        "{:<15} {:<24} {:<10} LAST_SEEN",
        "NAME", "ADDRESS", "REACHABLE"
    );

    for host in hosts {
        let address = format!("{}:{}", host.address, host.api_port);
        let reachable = if host.reachable { "yes" } else { "no" };
        let last_seen = host.last_seen.as_deref().unwrap_or("—");
        println!(
            "{:<15} {:<24} {:<10} {}",
            host.name, address, reachable, last_seen
        );
    }
}

pub fn print_health(status: &HealthResponse) {
    println!("status: {}", status.status);
    println!("version: {}", status.version);
    println!("uptime_secs: {}", status.uptime_secs);
    println!("session_count: {}", status.session_count);
    println!("db_path: {}", status.db_path);
}

pub fn print_action(result: &ActionResponse) {
    println!("{}", result.message);
}

pub fn print_json_pretty<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    let output = serde_json::to_string_pretty(value)?;
    println!("{output}");
    Ok(())
}

fn window_name(session: &SessionSummary) -> String {
    if session.status.eq_ignore_ascii_case("stopped") {
        return "—".to_string();
    }

    // Daemon API does not currently expose a dedicated tmux window name, so this
    // uses the active (or first available) pane name as the best proxy.
    let mut first_name: Option<&str> = None;
    for pane in &session.panes {
        if !pane.name.is_empty() && first_name.is_none() {
            first_name = Some(&pane.name);
        }

        if pane.status.eq_ignore_ascii_case("active") {
            return if pane.name.is_empty() {
                "—".to_string()
            } else {
                pane.name.clone()
            };
        }
    }

    first_name.unwrap_or("—").to_string()
}
