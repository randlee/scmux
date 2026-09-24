//! Session launch dispatch.
//!
//! Existing definitions continue to use tmuxp. Definitions may opt into an
//! argv-based external launcher when that launcher must perform additional
//! lifecycle work (for example, registering pane identities with ATM).

use anyhow::{bail, Context};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Tmuxp,
    External,
}

impl LaunchMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Tmuxp => "tmuxp",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalLaunch {
    argv: Vec<String>,
    working_directory: PathBuf,
}

pub fn launch_mode(config_json: &str) -> LaunchMode {
    if matches!(external_launch(config_json, ""), Ok(Some(_))) {
        LaunchMode::External
    } else {
        LaunchMode::Tmuxp
    }
}

pub fn external_working_directory(config_json: &str) -> anyhow::Result<Option<PathBuf>> {
    Ok(external_launch(config_json, "")?.map(|launch| launch.working_directory))
}

pub async fn start_session(name: &str, config_json: &str) -> anyhow::Result<LaunchMode> {
    let Some(launch) = external_launch(config_json, name)? else {
        crate::tmux::start_session(name, config_json).await?;
        return Ok(LaunchMode::Tmuxp);
    };

    let (program, arguments) = launch
        .argv
        .split_first()
        .context("launch.command must contain an executable")?;
    let program = resolve_program(program, &launch.working_directory);
    let output = Command::new(program)
        .args(arguments)
        .current_dir(&launch.working_directory)
        .output()
        .await
        .context("external launcher could not be executed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        bail!(
            "external launcher failed with status {}{}",
            output.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        );
    }

    Ok(LaunchMode::External)
}

fn external_launch(config_json: &str, name: &str) -> anyhow::Result<Option<ExternalLaunch>> {
    let value: Value = serde_json::from_str(config_json).context("invalid session config JSON")?;
    let Some(command) = value.pointer("/launch/command") else {
        return Ok(None);
    };
    let command = command
        .as_array()
        .context("launch.command must be an argv array")?;
    if command.is_empty() {
        bail!("launch.command must contain at least one item");
    }

    let mut argv = Vec::with_capacity(command.len());
    for item in command {
        let item = item
            .as_str()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .context("launch.command items must be non-empty strings")?;
        argv.push(item.replace("{team_name}", name));
    }

    let configured_root = value
        .pointer("/launch/working_directory")
        .or_else(|| value.get("root_path"))
        .or_else(|| value.get("start_directory"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .context(
            "launch.working_directory, root_path, or start_directory is required for launch.command",
        )?;
    let working_directory = expand_home(configured_root)?;
    if !working_directory.is_dir() {
        bail!(
            "external launcher working directory does not exist: {}",
            working_directory.display()
        );
    }

    Ok(Some(ExternalLaunch {
        argv,
        working_directory,
    }))
}

fn expand_home(value: &str) -> anyhow::Result<PathBuf> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .context("cannot expand launcher home directory")?;
        let suffix = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
            .unwrap_or("");
        return Ok(PathBuf::from(home).join(suffix));
    }
    Ok(PathBuf::from(value))
}

fn resolve_program(program: &str, working_directory: &Path) -> PathBuf {
    let path = Path::new(program);
    if path.is_absolute() || path.components().count() == 1 {
        path.to_path_buf()
    } else {
        working_directory.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn defaults_to_tmuxp_without_launch_command() {
        assert_eq!(
            launch_mode(r#"{"session_name":"alpha"}"#),
            LaunchMode::Tmuxp
        );
    }

    #[tokio::test]
    async fn external_launcher_receives_team_substitution_and_working_directory() {
        let root = tempfile::tempdir().expect("temp root");
        let output_path = root.path().join("proof.txt");
        let script_path = root.path().join("launcher.sh");
        let mut script = std::fs::File::create(&script_path).expect("create launcher");
        writeln!(
            script,
            "#!/bin/sh\nprintf '%s|%s' \"$1\" \"$PWD\" > '{}'",
            output_path.display()
        )
        .expect("write launcher");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = script.metadata().expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&script_path, permissions).expect("chmod launcher");
        }

        let config = serde_json::json!({
            "session_name": "alpha",
            "root_path": root.path(),
            "launch": {"command": [script_path, "{team_name}"]},
            "panes": [{"name": "agent"}]
        });
        let mode = start_session("alpha", &config.to_string())
            .await
            .expect("external launch");
        assert_eq!(mode, LaunchMode::External);
        let proof = std::fs::read_to_string(output_path).expect("read proof");
        let (team, cwd) = proof.split_once('|').expect("proof fields");
        assert_eq!(team, "alpha");
        assert_eq!(
            std::fs::canonicalize(cwd).expect("canonical proof cwd"),
            root.path().canonicalize().expect("canonical temp root")
        );
    }

    #[test]
    fn rejects_shell_string_launch_commands() {
        let config = serde_json::json!({
            "session_name": "alpha",
            "root_path": "/tmp",
            "launch": {"command": "echo unsafe"}
        });
        let error = external_launch(&config.to_string(), "alpha").expect_err("reject string");
        assert!(error.to_string().contains("argv array"));
    }
}
