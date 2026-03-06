use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub index: u32,
    pub name: String,
    pub status: String,       // active | idle | stopped
    pub last_activity: String,
    pub current_command: String,
}

/// Returns set of live tmux session names mapped to their panes
pub async fn live_sessions() -> anyhow::Result<HashMap<String, Vec<PaneInfo>>> {
    let out = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .await;

    let mut result: HashMap<String, Vec<PaneInfo>> = HashMap::new();

    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return Ok(result), // tmux not running or no sessions
    };

    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    for name in names {
        let panes = list_panes(&name).await.unwrap_or_default();
        result.insert(name, panes);
    }

    Ok(result)
}

async fn list_panes(session: &str) -> anyhow::Result<Vec<PaneInfo>> {
    let out = Command::new("tmux")
        .args([
            "list-panes", "-t", session, "-a",
            "-F",
            "#{pane_index}|#{pane_title}|#{pane_current_command}|#{pane_active}",
        ])
        .output()
        .await?;

    let panes = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, line)| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            let index = parts.first().and_then(|s| s.parse().ok()).unwrap_or(i as u32);
            let name = parts.get(1).unwrap_or(&"").to_string();
            let command = parts.get(2).unwrap_or(&"").to_string();
            let active = parts.get(3).map(|s| *s == "1").unwrap_or(false);
            let status = if active { "active" } else { "idle" }.to_string();

            PaneInfo {
                index,
                name: if name.is_empty() { format!("pane-{index}") } else { name },
                status,
                last_activity: "unknown".to_string(),
                current_command: command,
            }
        })
        .collect();

    Ok(panes)
}

pub async fn start_session(name: &str, config_json: &str) -> anyhow::Result<()> {
    // Write config to a temp file and load with tmuxp
    let tmp = std::env::temp_dir().join(format!("tms-{name}.json"));
    tokio::fs::write(&tmp, config_json).await?;

    let out = Command::new("tmuxp")
        .args(["load", "-d", tmp.to_str().unwrap()])
        .output()
        .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmuxp failed: {err}");
    }

    let _ = tokio::fs::remove_file(tmp).await;
    Ok(())
}

pub async fn stop_session(name: &str) -> anyhow::Result<()> {
    let out = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output()
        .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmux kill-session failed: {err}");
    }

    Ok(())
}
