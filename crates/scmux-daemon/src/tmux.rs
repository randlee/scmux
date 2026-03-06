use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub index: u32,
    pub name: String,
    pub status: String, // active | idle | stopped
    pub last_activity: String,
    pub current_command: String,
}

#[derive(Debug, Clone)]
pub enum HostTarget {
    Local,
    Remote { user: String, host: String },
}

/// Returns set of live tmux session names mapped to their panes.
pub async fn live_sessions() -> anyhow::Result<HashMap<String, Vec<PaneInfo>>> {
    let out = Command::new(tmux_bin())
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .await;

    let mut result: HashMap<String, Vec<PaneInfo>> = HashMap::new();
    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return Ok(result), // tmux not running or no sessions
    };

    let names = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    for name in names {
        let panes = list_panes(&name).await.unwrap_or_default();
        result.insert(name, panes);
    }

    Ok(result)
}

async fn list_panes(session: &str) -> anyhow::Result<Vec<PaneInfo>> {
    let out = Command::new(tmux_bin())
        .args([
            "list-panes",
            "-t",
            session,
            "-a",
            "-F",
            "#{pane_index}|#{pane_title}|#{pane_current_command}|#{pane_active}",
        ])
        .output()
        .await?;

    let panes = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(i, line)| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            let index = parts
                .first()
                .and_then(|s| s.parse().ok())
                .unwrap_or(i as u32);
            let pane_title = parts.get(1).copied().unwrap_or_default();
            let command = parts.get(2).copied().unwrap_or_default().to_string();
            let active = parts.get(3).map(|s| *s == "1").unwrap_or(false);
            let status = if active { "active" } else { "idle" }.to_string();

            PaneInfo {
                index,
                name: if pane_title.is_empty() {
                    format!("pane-{index}")
                } else {
                    pane_title.to_string()
                },
                status,
                last_activity: "unknown".to_string(),
                current_command: command,
            }
        })
        .collect();

    Ok(panes)
}

pub async fn start_session(name: &str, config_json: &str) -> anyhow::Result<()> {
    // Write config to a temp file and load with tmuxp.
    let tmp = std::env::temp_dir().join(format!("scmux-{name}.json"));
    tokio::fs::write(&tmp, config_json).await?;

    let out = Command::new(tmuxp_bin())
        .args(["load", "-d", tmp.to_str().unwrap_or_default()])
        .output()
        .await?;

    let _ = tokio::fs::remove_file(tmp).await;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmuxp failed: {err}");
    }

    Ok(())
}

pub async fn stop_session(name: &str) -> anyhow::Result<()> {
    let out = Command::new(tmux_bin())
        .args(["kill-session", "-t", name])
        .output()
        .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmux kill-session failed: {err}");
    }

    Ok(())
}

pub async fn jump_session(
    host: HostTarget,
    session: &str,
    terminal: &str,
) -> anyhow::Result<String> {
    if !terminal.eq_ignore_ascii_case("iterm2") {
        anyhow::bail!("unsupported terminal '{terminal}'");
    }

    let escaped_session = shell_escape(session);
    let command = match host {
        HostTarget::Local => format!("tmux attach -t {escaped_session}"),
        HostTarget::Remote { user, host } => {
            format!(
                "ssh {}@{} tmux attach -t {escaped_session}",
                shell_escape(&user),
                shell_escape(&host)
            )
        }
    };
    let script = format!(
        "tell application \"iTerm2\"\n  create window with default profile\n  tell current session of current window\n    write text \"{}\"\n  end tell\nend tell",
        apple_script_escape(&command)
    );

    let out = Command::new(osascript_bin())
        .args(["-e", script.as_str()])
        .output()
        .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let message = if err.is_empty() {
            "osascript failed to launch iTerm2".to_string()
        } else {
            format!("osascript failed: {err}")
        };
        anyhow::bail!(message);
    }

    Ok("launched iTerm2".to_string())
}

fn tmux_bin() -> String {
    std::env::var("SCMUX_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
}

fn tmuxp_bin() -> String {
    std::env::var("SCMUX_TMUXP_BIN").unwrap_or_else(|_| "tmuxp".to_string())
}

fn osascript_bin() -> String {
    std::env::var("SCMUX_OSASCRIPT_BIN").unwrap_or_else(|_| "osascript".to_string())
}

fn shell_escape(input: &str) -> String {
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '@'))
    {
        return input.to_string();
    }
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

fn apple_script_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    fn write_script(contents: &str) -> tempfile::TempPath {
        let mut file = tempfile::NamedTempFile::new().expect("temp script");
        file.write_all(contents.as_bytes()).expect("write script");
        let mut perms = file.as_file().metadata().expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            file.as_file().set_permissions(perms).expect("chmod");
        }
        file.into_temp_path()
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "lock held across await intentionally; restructure deferred to Phase 3"
    )]
    async fn td_09_live_sessions_parses_session_names_correctly() {
        let _guard = with_env_lock();
        let script = write_script(
            r#"#!/bin/sh
if [ "$1" = "list-sessions" ]; then
  echo "alpha"
  echo "beta"
  exit 0
fi
if [ "$1" = "list-panes" ]; then
  if [ "$3" = "alpha" ]; then
    echo "0|lead|zsh|1"
    echo "1|worker|vim|0"
    exit 0
  fi
  echo "0|solo|bash|1"
  exit 0
fi
exit 1
"#,
        );
        // SAFETY: test-only env mutation under global lock.
        unsafe { std::env::set_var("SCMUX_TMUX_BIN", script.to_string_lossy().to_string()) };

        let sessions = live_sessions().await.expect("live sessions");
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains_key("alpha"));
        assert!(sessions.contains_key("beta"));
        assert_eq!(sessions["alpha"].len(), 2);
        assert_eq!(sessions["alpha"][0].status, "active");
        assert_eq!(sessions["alpha"][1].status, "idle");

        // SAFETY: test teardown under global lock.
        unsafe { std::env::remove_var("SCMUX_TMUX_BIN") };
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "lock held across await intentionally; restructure deferred to Phase 3"
    )]
    async fn td_08_live_sessions_returns_empty_when_tmux_not_running() {
        let _guard = with_env_lock();
        let script = write_script("#!/bin/sh\nexit 1\n");
        // SAFETY: test-only env mutation under global lock.
        unsafe { std::env::set_var("SCMUX_TMUX_BIN", script.to_string_lossy().to_string()) };

        let sessions = live_sessions().await.expect("live sessions");
        assert!(sessions.is_empty());

        // SAFETY: test teardown under global lock.
        unsafe { std::env::remove_var("SCMUX_TMUX_BIN") };
    }
}
