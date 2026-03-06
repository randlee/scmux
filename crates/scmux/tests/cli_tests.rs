use clap::Parser;
use scmux::client::resolve_base_url;
use scmux::{Cli, Command, DaemonCommand};
use std::sync::{Mutex, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn set_env_var(key: &str, value: &str) -> Option<String> {
    let prev = std::env::var(key).ok();
    // SAFETY: test-only env mutation under global test mutex.
    unsafe { std::env::set_var(key, value) };
    prev
}

fn restore_env_var(key: &str, prev: Option<String>) {
    match prev {
        Some(value) => {
            // SAFETY: test-only env restoration under global test mutex.
            unsafe { std::env::set_var(key, value) };
        }
        None => {
            // SAFETY: test-only env restoration under global test mutex.
            unsafe { std::env::remove_var(key) };
        }
    }
}

#[test]
fn td_c_01_parse_list_command() {
    let cli =
        Cli::try_parse_from(["scmux", "list", "--project", "demo"]).expect("parse list command");
    match cli.command {
        Command::List { project } => assert_eq!(project.as_deref(), Some("demo")),
        other => panic!("expected list command, got {other:?}"),
    }
}

#[test]
fn td_c_02_parse_daemon_status_command() {
    let cli = Cli::try_parse_from(["scmux", "daemon", "status"]).expect("parse daemon status");
    match cli.command {
        Command::Daemon { command } => assert!(matches!(command, DaemonCommand::Status)),
        other => panic!("expected daemon command, got {other:?}"),
    }
}

#[test]
fn td_c_03_parse_add_command() {
    let cli = Cli::try_parse_from([
        "scmux",
        "add",
        "--name",
        "alpha",
        "--project",
        "demo",
        "--config",
        "alpha.json",
        "--auto-start",
    ])
    .expect("parse add command");

    match cli.command {
        Command::Add {
            name,
            project,
            config,
            auto_start,
            ..
        } => {
            assert_eq!(name, "alpha");
            assert_eq!(project.as_deref(), Some("demo"));
            assert_eq!(config, "alpha.json");
            assert!(auto_start);
        }
        other => panic!("expected add command, got {other:?}"),
    }
}

#[test]
fn td_c_04_host_resolution_uses_default_when_no_env_or_flag() {
    let _guard = env_lock().lock().expect("env lock");
    let prev = std::env::var("SCMUX_HOST").ok();
    // SAFETY: test-only env mutation under global test mutex.
    unsafe { std::env::remove_var("SCMUX_HOST") };

    let resolved = resolve_base_url(None);

    restore_env_var("SCMUX_HOST", prev);
    assert_eq!(resolved, "http://localhost:7700");
}

#[test]
fn td_c_05_host_resolution_uses_flag_when_env_missing() {
    let _guard = env_lock().lock().expect("env lock");
    let prev = std::env::var("SCMUX_HOST").ok();
    // SAFETY: test-only env mutation under global test mutex.
    unsafe { std::env::remove_var("SCMUX_HOST") };

    let resolved = resolve_base_url(Some("127.0.0.1:8800"));

    restore_env_var("SCMUX_HOST", prev);
    assert_eq!(resolved, "http://127.0.0.1:8800");
}

#[test]
fn td_c_06_host_resolution_env_overrides_flag() {
    let _guard = env_lock().lock().expect("env lock");
    let prev = set_env_var("SCMUX_HOST", "https://example.internal:9999");

    let resolved = resolve_base_url(Some("127.0.0.1:8800"));

    restore_env_var("SCMUX_HOST", prev);
    assert_eq!(resolved, "https://example.internal:9999");
}
