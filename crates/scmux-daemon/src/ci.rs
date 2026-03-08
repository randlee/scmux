use crate::{db, runtime::CiRuntimeSummary, AppState};
use chrono::Utc;
use serde::Serialize;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

const GH_INSTALL_HINT: &str = "Install gh CLI: brew install gh";
const AZ_INSTALL_HINT: &str = "Install az CLI: brew install azure-cli";

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolAvailability {
    pub gh_available: bool,
    pub az_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResult {
    pub status: String,
    pub data: Option<serde_json::Value>,
    pub tool_message: Option<String>,
}

pub fn detect_tools() -> ToolAvailability {
    ToolAvailability {
        gh_available: detect_tool(&gh_bin()),
        az_available: detect_tool(&az_bin()),
    }
}

pub fn next_interval(has_active_pane: bool) -> Duration {
    if has_active_pane {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(300)
    }
}

pub async fn poll_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let sessions = {
        let state = Arc::clone(state);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<db::SessionDefinition>> {
            let db_conn = state.db.lock().expect("db lock");
            db::list_sessions_for_host(&db_conn, state.host_id)
        })
        .await??
    };

    for session in sessions {
        if let Some(repo) = session.github_repo.as_deref() {
            poll_provider(
                Arc::clone(state),
                &session,
                "github",
                state.ci_tools.gh_available,
                GH_INSTALL_HINT,
                poll_github(repo),
            )
            .await;
        }
        if let Some(project) = session.azure_project.as_deref() {
            poll_provider(
                Arc::clone(state),
                &session,
                "azure",
                state.ci_tools.az_available,
                AZ_INSTALL_HINT,
                poll_azure(project),
            )
            .await;
        }
    }

    Ok(())
}

pub async fn poll_github(repo: &str) -> ProviderResult {
    let prs = run_json_command(
        &gh_bin(),
        &[
            "pr",
            "list",
            "--repo",
            repo,
            "--json",
            "number,title,url,author,isDraft",
        ],
    )
    .await;
    let runs = run_json_command(
        &gh_bin(),
        &[
            "run",
            "list",
            "--repo",
            repo,
            "--json",
            "status,conclusion,headBranch,createdAt,displayTitle",
            "--limit",
            "10",
        ],
    )
    .await;

    match (prs, runs) {
        (Ok(prs_json), Ok(runs_json)) => ProviderResult {
            status: "ok".to_string(),
            data: Some(serde_json::json!({
                "prs": prs_json,
                "runs": runs_json,
            })),
            tool_message: None,
        },
        (Err(err), _) | (_, Err(err)) => classify_cli_error(err),
    }
}

pub async fn poll_azure(project: &str) -> ProviderResult {
    let prs = run_json_command(
        &az_bin(),
        &[
            "repos",
            "pr",
            "list",
            "--project",
            project,
            "--status",
            "active",
            "--output",
            "json",
        ],
    )
    .await;
    let runs = run_json_command(
        &az_bin(),
        &[
            "pipelines",
            "runs",
            "list",
            "--project",
            project,
            "--top",
            "10",
            "--output",
            "json",
        ],
    )
    .await;

    match (prs, runs) {
        (Ok(prs_json), Ok(runs_json)) => ProviderResult {
            status: "ok".to_string(),
            data: Some(serde_json::json!({
                "prs": prs_json,
                "runs": runs_json,
            })),
            tool_message: None,
        },
        (Err(err), _) | (_, Err(err)) => classify_cli_error(err),
    }
}

async fn poll_provider(
    state: Arc<AppState>,
    session: &db::SessionDefinition,
    provider: &str,
    tool_available: bool,
    tool_hint: &str,
    poll_result: impl std::future::Future<Output = ProviderResult>,
) {
    let has_active_pane = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime
            .session(&session.name)
            .map(|row| {
                row.panes
                    .iter()
                    .any(|pane| pane.status.eq_ignore_ascii_case("active"))
            })
            .unwrap_or(false)
    };

    let now = Utc::now();
    let due = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.ci_due(session.id, provider, now)
    };
    if !due {
        return;
    }

    let polled_at = now.to_rfc3339();
    let next_due = now
        + chrono::Duration::from_std(next_interval(has_active_pane))
            .unwrap_or_else(|_| chrono::Duration::minutes(5));

    let result = if !tool_available {
        ProviderResult {
            status: "tool_unavailable".to_string(),
            data: None,
            tool_message: Some(tool_hint.to_string()),
        }
    } else {
        poll_result.await
    };

    let summary = CiRuntimeSummary {
        provider: provider.to_string(),
        status: result.status,
        data_json: result.data,
        tool_message: result.tool_message,
        polled_at: Some(polled_at),
        next_poll_at: Some(next_due.to_rfc3339()),
    };

    let mut runtime = state.runtime.lock().expect("runtime lock");
    runtime.upsert_ci(&session.name, session.id, summary, next_due);
}

async fn run_json_command(bin: &str, args: &[&str]) -> anyhow::Result<serde_json::Value> {
    let output = tokio::process::Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(if stderr.is_empty() {
            format!("{bin} command failed with status {}", output.status)
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(serde_json::from_str(stdout.trim())?)
}

fn detect_tool(bin: &str) -> bool {
    std::process::Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn classify_cli_error(err: anyhow::Error) -> ProviderResult {
    let message = err.to_string();
    let lower = message.to_lowercase();
    let status = if lower.contains("rate limit") {
        "rate_limited"
    } else if lower.contains("auth")
        || lower.contains("authentication")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("login")
    {
        "auth_error"
    } else {
        "error"
    };
    ProviderResult {
        status: status.to_string(),
        data: None,
        tool_message: Some(message),
    }
}

fn gh_bin() -> String {
    std::env::var("SCMUX_GH_BIN").unwrap_or_else(|_| "gh".to_string())
}

fn az_bin() -> String {
    std::env::var("SCMUX_AZ_BIN").unwrap_or_else(|_| "az".to_string())
}
