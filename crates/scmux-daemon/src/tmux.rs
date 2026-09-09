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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTarget {
    Local,
    Remote { user: String, host: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JumpDestination {
    session: String,
    host: HostTarget,
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
    if !terminal.eq_ignore_ascii_case("iterm2") && !terminal.eq_ignore_ascii_case("terminal") {
        anyhow::bail!("unsupported terminal '{terminal}'");
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = host;
        let _ = session;
        anyhow::bail!("iTerm2 jump is only supported on macOS");
    }

    #[cfg(target_os = "macos")]
    {
        let destination = resolve_jump_destination(host, session, &local_host_aliases());
        let escaped_session = shell_escape(&destination.session);
        let command = match destination.host {
            HostTarget::Local => format!("tmux attach -t {escaped_session}"),
            HostTarget::Remote { user, host } => {
                format!(
                    "ssh {}@{} tmux attach -t {escaped_session}",
                    shell_escape(&user),
                    shell_escape(&host)
                )
            }
        };
        let escaped_command = apple_script_escape(&command);
        // AppleScript against iTerm2 only compiles when iTerm2 is installed
        // (a missing app means a missing scripting dictionary, surfacing as a
        // bare osascript syntax error), and any Apple Event sent from a
        // launchd daemon additionally needs a TCC Automation grant. Try
        // iTerm2 only when it is actually installed; otherwise (or on
        // failure) fall back to opening a .command file via LaunchServices,
        // which sends no Apple Events and needs no grant.
        if terminal.eq_ignore_ascii_case("iterm2") && iterm2_installed() {
            let script = format!(
                "tell application \"iTerm2\"\n  create window with default profile\n  tell current session of current window\n    write text \"{escaped_command}\"\n  end tell\nend tell"
            );
            let iterm = run_applescript(&script).await?;
            if iterm.status.success() {
                return Ok("launched iTerm2".to_string());
            }
        }

        match launch_via_command_file(&destination.session, &command).await {
            Ok(()) => Ok("launched Terminal".to_string()),
            Err(open_err) => {
                // Last resort: classic AppleScript path (works when the user
                // has granted the daemon Automation access to Terminal).
                let script = format!(
                    "tell application \"Terminal\"\n  activate\n  do script \"{escaped_command}\"\nend tell"
                );
                let output = run_applescript(&script).await?;
                if !output.status.success() {
                    anyhow::bail!(
                        "failed to launch Terminal via .command file ({open_err}) and via AppleScript ({})",
                        applescript_error(&output, "Terminal")
                    );
                }
                Ok("launched Terminal (AppleScript)".to_string())
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn iterm2_installed() -> bool {
    // Test/ops override: "1"/"0" forces the answer without touching the disk.
    if let Ok(forced) = std::env::var("SCMUX_ITERM_INSTALLED") {
        return forced == "1";
    }
    if std::path::Path::new("/Applications/iTerm.app").exists() {
        return true;
    }
    match std::env::var("HOME") {
        Ok(home) => std::path::Path::new(&format!("{home}/Applications/iTerm.app")).exists(),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn open_bin() -> String {
    std::env::var("SCMUX_OPEN_BIN").unwrap_or_else(|_| "/usr/bin/open".to_string())
}

/// Launch Terminal by opening an executable `.command` file through
/// LaunchServices. Unlike `osascript`, `open` sends no Apple Events, so it
/// works from a launchd daemon without any TCC Automation grant.
#[cfg(target_os = "macos")]
async fn launch_via_command_file(session: &str, command: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let safe_name: String = session
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join("scmux-jump");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("attach-{safe_name}.command"));

    let mut file = std::fs::File::create(&path)?;
    writeln!(file, "#!/bin/zsh")?;
    writeln!(file, "exec {command}")?;
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;

    let out = Command::new(open_bin()).arg(&path).output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!("open {} failed: {stderr}", path.display());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn run_applescript(script: &str) -> anyhow::Result<std::process::Output> {
    Ok(Command::new(osascript_bin())
        .args(["-e", script])
        .output()
        .await?)
}

#[cfg(target_os = "macos")]
fn applescript_error(output: &std::process::Output, application: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("osascript could not launch {application}")
    } else {
        stderr
    }
}

/// Resolve an optional host-qualified team name.
///
/// Supported forms are `<session>@local` and `<session>@<ssh-user>@<host>`.
/// The qualifier is routing metadata only and is removed from the tmux session
/// name. Unqualified names retain the host selected from the scmux registry.
fn resolve_jump_destination(
    configured_host: HostTarget,
    qualified_name: &str,
    local_aliases: &[String],
) -> JumpDestination {
    if let Some(session) = qualified_name.strip_suffix("@local") {
        if !session.is_empty() && !session.contains('@') {
            return JumpDestination {
                session: session.to_string(),
                host: HostTarget::Local,
            };
        }
    }

    if let Some((session_and_user, host)) = qualified_name.rsplit_once('@') {
        if let Some((session, user)) = session_and_user.rsplit_once('@') {
            if !session.is_empty() && !user.is_empty() && !host.is_empty() {
                let host_target = if host_matches_local(host, local_aliases) {
                    HostTarget::Local
                } else {
                    HostTarget::Remote {
                        user: user.to_string(),
                        host: host.to_string(),
                    }
                };
                return JumpDestination {
                    session: session.to_string(),
                    host: host_target,
                };
            }
        }
    }

    JumpDestination {
        session: qualified_name.to_string(),
        host: configured_host,
    }
}

fn local_host_aliases() -> Vec<String> {
    let mut aliases = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();

    if let Ok(output) = std::process::Command::new("hostname").output() {
        if output.status.success() {
            let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !hostname.is_empty() {
                aliases.push(hostname);
            }
        }
    }
    aliases
}

fn host_matches_local(host: &str, local_aliases: &[String]) -> bool {
    let literal = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if matches!(
        literal.as_str(),
        "local" | "localhost" | "127.0.0.1" | "::1"
    ) {
        return true;
    }
    let expected = normalized_host(host);
    local_aliases
        .iter()
        .any(|alias| normalized_host(alias) == expected)
}

fn normalized_host(host: &str) -> String {
    host.trim()
        .trim_end_matches('.')
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn tmux_bin() -> String {
    std::env::var("SCMUX_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
}

fn tmuxp_bin() -> String {
    std::env::var("SCMUX_TMUXP_BIN").unwrap_or_else(|_| "tmuxp".to_string())
}

#[cfg(target_os = "macos")]
fn osascript_bin() -> String {
    std::env::var("SCMUX_OSASCRIPT_BIN").unwrap_or_else(|_| "osascript".to_string())
}

#[cfg(target_os = "macos")]
fn shell_escape(input: &str) -> String {
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '@'))
    {
        return input.to_string();
    }
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod jump_tests {
    use super::*;

    fn remote() -> HostTarget {
        HostTarget::Remote {
            user: "configured-user".to_string(),
            host: "configured-host".to_string(),
        }
    }

    #[test]
    fn unqualified_name_retains_configured_host() {
        let destination = resolve_jump_destination(remote(), "aidw-dev", &[]);
        assert_eq!(destination.session, "aidw-dev");
        assert!(matches!(
            destination.host,
            HostTarget::Remote { ref user, ref host }
                if user == "configured-user" && host == "configured-host"
        ));
    }

    #[test]
    fn local_qualifier_selects_local_host_and_strips_routing_suffix() {
        let destination = resolve_jump_destination(remote(), "hitl-dev@local", &[]);
        assert_eq!(destination.session, "hitl-dev");
        assert!(matches!(destination.host, HostTarget::Local));
    }

    #[test]
    fn remote_qualifier_builds_ssh_target_and_strips_routing_suffix() {
        let destination = resolve_jump_destination(
            HostTarget::Local,
            "aidw-dev@aidwlead@HITL2",
            &["Radiant-MTP-MacBook-Pro.local".to_string()],
        );
        assert_eq!(destination.session, "aidw-dev");
        assert!(matches!(
            destination.host,
            HostTarget::Remote { ref user, ref host }
                if user == "aidwlead" && host == "HITL2"
        ));
    }

    #[test]
    fn computer_id_matching_current_hostname_skips_ssh() {
        let destination = resolve_jump_destination(
            remote(),
            "aidw-dev@aidwlead@HITL2",
            &["hitl2.local".to_string()],
        );
        assert_eq!(destination.session, "aidw-dev");
        assert!(matches!(destination.host, HostTarget::Local));
    }

    #[test]
    fn loopback_computer_ids_never_use_ssh() {
        for computer_id in ["local", "localhost", "127.0.0.1", "::1"] {
            let name = format!("aidw-dev@aidwlead@{computer_id}");
            let destination = resolve_jump_destination(remote(), &name, &[]);
            assert_eq!(destination.session, "aidw-dev");
            assert!(matches!(destination.host, HostTarget::Local));
        }
    }

    #[test]
    fn malformed_qualifier_remains_a_legacy_session_name() {
        let destination = resolve_jump_destination(HostTarget::Local, "aidw-dev@HITL2", &[]);
        assert_eq!(destination.session, "aidw-dev@HITL2");
        assert!(matches!(destination.host, HostTarget::Local));
    }
}

#[cfg(target_os = "macos")]
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

    #[tokio::test]
    #[cfg(target_os = "macos")]
    #[expect(
        clippy::await_holding_lock,
        reason = "test-only process environment is serialized across the awaited launch"
    )]
    async fn host_qualified_jump_emits_ssh_before_tmux_attach() {
        let _guard = with_env_lock();
        let output = tempfile::NamedTempFile::new().expect("output file");
        // Capture the launched .command file's contents instead of Apple Events.
        let script = write_script(&format!(
            "#!/bin/sh\ncat \"$1\" > '{}'\n",
            output.path().display()
        ));
        // SAFETY: test-only env mutation under global lock.
        unsafe { std::env::set_var("SCMUX_OPEN_BIN", script.to_string_lossy().to_string()) };
        unsafe { std::env::set_var("SCMUX_ITERM_INSTALLED", "0") };

        jump_session(
            HostTarget::Local,
            "aidw-dev@aidwlead@scmux-remote-test.invalid",
            "iterm2",
        )
        .await
        .expect("jump command");

        // SAFETY: test teardown under global lock.
        unsafe { std::env::remove_var("SCMUX_OPEN_BIN") };
        unsafe { std::env::remove_var("SCMUX_ITERM_INSTALLED") };
        let args = std::fs::read_to_string(output.path()).expect("captured .command contents");
        assert!(args.contains("ssh aidwlead@scmux-remote-test.invalid tmux attach -t aidw-dev"));
        assert!(!args.contains("tmux attach -t aidw-dev@aidwlead@"));
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    #[expect(
        clippy::await_holding_lock,
        reason = "test-only process environment is serialized across the awaited launch"
    )]
    async fn local_jump_uses_operating_system_without_ssh() {
        let _guard = with_env_lock();
        let output = tempfile::NamedTempFile::new().expect("output file");
        // Capture the launched .command file's contents instead of Apple Events.
        let script = write_script(&format!(
            "#!/bin/sh\ncat \"$1\" > '{}'\n",
            output.path().display()
        ));
        // SAFETY: test-only env mutation under global lock.
        unsafe { std::env::set_var("SCMUX_OPEN_BIN", script.to_string_lossy().to_string()) };
        unsafe { std::env::set_var("SCMUX_ITERM_INSTALLED", "0") };

        jump_session(
            HostTarget::Remote {
                user: "unused".to_string(),
                host: "unused".to_string(),
            },
            "hitl-dev@unused@localhost",
            "iterm2",
        )
        .await
        .expect("local jump command");

        // SAFETY: test teardown under global lock.
        unsafe { std::env::remove_var("SCMUX_OPEN_BIN") };
        unsafe { std::env::remove_var("SCMUX_ITERM_INSTALLED") };
        let args = std::fs::read_to_string(output.path()).expect("captured .command contents");
        assert!(args.contains("tmux attach -t hitl-dev"));
        assert!(!args.contains("ssh "));
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    #[expect(
        clippy::await_holding_lock,
        reason = "test-only process environment is serialized across the awaited launch"
    )]
    async fn missing_iterm_falls_back_to_terminal_without_networking() {
        let _guard = with_env_lock();
        let output = tempfile::NamedTempFile::new().expect("output file");
        // iTerm2 forced absent: jump must go straight to the .command file
        // path (no Apple Events at all). The fake osascript would fail loudly
        // if it were ever invoked.
        let osascript = write_script("#!/bin/sh\necho 'osascript must not run' 1>&2; exit 1\n");
        let script = write_script(&format!(
            "#!/bin/sh\ncat \"$1\" > '{}'\n",
            output.path().display()
        ));
        // SAFETY: test-only env mutation under global lock.
        unsafe { std::env::set_var("SCMUX_OSASCRIPT_BIN", osascript.to_string_lossy().to_string()) };
        unsafe { std::env::set_var("SCMUX_OPEN_BIN", script.to_string_lossy().to_string()) };
        unsafe { std::env::set_var("SCMUX_ITERM_INSTALLED", "0") };

        let message = jump_session(HostTarget::Local, "hitl-dev@local", "iterm2")
            .await
            .expect("Terminal fallback");

        // SAFETY: test teardown under global lock.
        unsafe { std::env::remove_var("SCMUX_OSASCRIPT_BIN") };
        unsafe { std::env::remove_var("SCMUX_OPEN_BIN") };
        unsafe { std::env::remove_var("SCMUX_ITERM_INSTALLED") };
        assert_eq!(message, "launched Terminal");
        let args = std::fs::read_to_string(output.path()).expect("captured .command contents");
        assert!(args.contains("tmux attach -t hitl-dev"));
        assert!(!args.contains("ssh "));
    }
}
