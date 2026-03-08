//! scmux CLI crate.
//!
//! This crate contains the command-line surface, daemon HTTP client, and
//! terminal output helpers for the `scmux` binary.

pub mod client;
pub mod output;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "scmux")]
#[command(about = "CLI client for scmux-daemon")]
pub struct Cli {
    /// Daemon base URL (e.g. http://localhost:7878)
    #[arg(long, global = true)]
    pub host: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List sessions
    List {
        /// Optional project filter
        #[arg(long)]
        project: Option<String>,
    },
    /// Show full session detail
    Show { name: String },
    /// Start a session
    Start { name: String },
    /// Stop a session
    Stop { name: String },
    /// Jump to a session via daemon terminal launch
    Jump {
        name: String,
        #[arg(long)]
        terminal: Option<String>,
        #[arg(long)]
        host_id: Option<i64>,
    },
    /// Session definition write commands (enabled in Sprint 6.cli)
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Host definition write commands (enabled in Sprint 6.cli)
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    /// Register a new session
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        config: String,
        #[arg(long)]
        cron: Option<String>,
        #[arg(long)]
        auto_start: bool,
        #[arg(long)]
        host_id: Option<i64>,
        #[arg(long)]
        github_repo: Option<String>,
        #[arg(long)]
        azure_project: Option<String>,
    },
    /// Edit a session
    Edit {
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        config: Option<String>,
        #[arg(long)]
        cron: Option<String>,
        /// Set auto-start behavior. Supports `--auto-start` and `--auto-start=false`.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        auto_start: Option<bool>,
        #[arg(long)]
        github_repo: Option<String>,
        #[arg(long)]
        azure_project: Option<String>,
    },
    /// Disable a session
    Disable { name: String },
    /// Enable a session
    Enable { name: String },
    /// Remove a session
    Remove { name: String },
    /// List hosts
    Hosts,
    /// Daemon subcommands
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Comprehensive runtime diagnostics
    Doctor,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Show daemon health status
    Status,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Create a session definition
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        config: String,
        #[arg(long)]
        cron: Option<String>,
        #[arg(long)]
        auto_start: bool,
        #[arg(long)]
        host_id: Option<i64>,
        #[arg(long)]
        github_repo: Option<String>,
        #[arg(long)]
        azure_project: Option<String>,
    },
    /// Update a session definition
    Edit {
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        config: Option<String>,
        #[arg(long)]
        cron: Option<String>,
        /// Set auto-start behavior. Supports `--auto-start` and `--auto-start=false`.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        auto_start: Option<bool>,
        #[arg(long)]
        github_repo: Option<String>,
        #[arg(long)]
        azure_project: Option<String>,
    },
    /// Disable a session definition
    Disable { name: String },
    /// Enable a session definition
    Enable { name: String },
    /// Remove a session definition
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Create a host definition
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        address: String,
        #[arg(long)]
        ssh_user: Option<String>,
        #[arg(long)]
        api_port: Option<u16>,
        #[arg(long)]
        is_local: Option<bool>,
    },
    /// Update a host definition
    Edit {
        id: i64,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        address: Option<String>,
        #[arg(long)]
        ssh_user: Option<String>,
        #[arg(long)]
        clear_ssh_user: bool,
        #[arg(long)]
        api_port: Option<u16>,
    },
    /// Remove a host definition
    Remove { id: i64 },
}
