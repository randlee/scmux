use anyhow::anyhow;
use clap::Parser;
use scmux::client::{
    resolve_base_url, ActionResponse, ApiClient, ClientError, CreateSessionRequest, JumpRequest,
    PatchSessionRequest,
};
use scmux::output;
use scmux::{Cli, Command, DaemonCommand};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli).await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let client = ApiClient::new(resolve_base_url(cli.host.as_deref()));

    match cli.command {
        Command::List { project } => {
            let mut sessions = client.list_sessions().await.map_err(map_client_error)?;
            if let Some(project_filter) = project {
                sessions
                    .retain(|session| session.project.as_deref() == Some(project_filter.as_str()));
            }

            let hosts = client.list_hosts().await.map_err(map_client_error)?;
            output::print_session_list(&sessions, &hosts);
        }
        Command::Show { name } => {
            let session = client
                .get_session(&name)
                .await
                .map_err(|err| map_session_error(err, &name))?;
            output::print_json_pretty(&session)?;
        }
        Command::Start { name } => {
            let action = client
                .start_session(&name)
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Stop { name } => {
            let action = client
                .stop_session(&name)
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Jump {
            name,
            terminal,
            host_id,
        } => {
            let action = client
                .jump_session(&name, &JumpRequest { terminal, host_id })
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Add {
            name,
            project,
            config,
            cron,
            auto_start,
            host_id,
            github_repo,
            azure_project,
        } => {
            let config_json = read_json_file(&config)?;
            let action = client
                .create_session(&CreateSessionRequest {
                    name,
                    project,
                    config_json,
                    cron_schedule: cron,
                    auto_start: Some(auto_start),
                    host_id,
                    github_repo,
                    azure_project,
                })
                .await
                .map_err(map_client_error)?;
            ensure_action_ok(action)?;
        }
        Command::Edit {
            name,
            project,
            config,
            cron,
            auto_start,
            github_repo,
            azure_project,
        } => {
            let mut patch = PatchSessionRequest::default();
            if let Some(project) = project {
                patch.project = Some(Some(project));
            }
            if let Some(config_path) = config {
                patch.config_json = Some(read_json_file(&config_path)?);
            }
            if let Some(cron_expr) = cron {
                patch.cron_schedule = Some(Some(cron_expr));
            }
            if let Some(auto_start) = auto_start {
                patch.auto_start = Some(auto_start);
            }
            if let Some(github_repo) = github_repo {
                patch.github_repo = Some(Some(github_repo));
            }
            if let Some(azure_project) = azure_project {
                patch.azure_project = Some(Some(azure_project));
            }

            if patch.is_empty() {
                return Err(anyhow!("scmux: no changes requested"));
            }

            let action = client
                .patch_session(&name, &patch)
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Disable { name } => {
            let action = client
                .patch_session(
                    &name,
                    &PatchSessionRequest {
                        enabled: Some(false),
                        ..PatchSessionRequest::default()
                    },
                )
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Enable { name } => {
            let action = client
                .patch_session(
                    &name,
                    &PatchSessionRequest {
                        enabled: Some(true),
                        ..PatchSessionRequest::default()
                    },
                )
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Remove { name } => {
            let action = client
                .delete_session(&name)
                .await
                .map_err(|err| map_session_error(err, &name))?;
            ensure_action_ok(action)?;
        }
        Command::Hosts => {
            let hosts = client.list_hosts().await.map_err(map_client_error)?;
            output::print_hosts(&hosts);
        }
        Command::Daemon { command } => match command {
            DaemonCommand::Status => {
                let health = client.health().await.map_err(map_client_error)?;
                output::print_health(&health);
            }
        },
    }

    Ok(())
}

fn ensure_action_ok(action: ActionResponse) -> anyhow::Result<()> {
    if action.ok {
        output::print_action(&action);
        Ok(())
    } else {
        Err(anyhow!("scmux: {}", action.message))
    }
}

fn read_json_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let contents = std::fs::read_to_string(path)
        .map_err(|err| anyhow!("scmux: failed to read config file '{path}': {err}"))?;
    serde_json::from_str(&contents)
        .map_err(|err| anyhow!("scmux: invalid JSON in config file '{path}': {err}"))
}

fn map_session_error(err: ClientError, name: &str) -> anyhow::Error {
    match err {
        ClientError::NotFound => anyhow!("scmux: session {name} not found"),
        other => map_client_error(other),
    }
}

fn map_client_error(err: ClientError) -> anyhow::Error {
    anyhow::Error::new(err)
}
