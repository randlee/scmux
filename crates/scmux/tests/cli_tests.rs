use clap::Parser;
use scmux::client::resolve_base_url;
use scmux::{Cli, Command, DaemonCommand, HostCommand, SessionCommand};
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
    assert_eq!(resolved, "http://localhost:7878");
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
fn td_c_06_host_resolution_flag_overrides_env() {
    let _guard = env_lock().lock().expect("env lock");
    let prev = set_env_var("SCMUX_HOST", "https://example.internal:9999");

    let resolved = resolve_base_url(Some("127.0.0.1:8800"));

    restore_env_var("SCMUX_HOST", prev);
    assert_eq!(resolved, "http://127.0.0.1:8800");
}

#[test]
fn td_c_07_parse_edit_auto_start_false() {
    let cli = Cli::try_parse_from(["scmux", "edit", "alpha", "--auto-start=false"])
        .expect("parse edit auto-start false");

    match cli.command {
        Command::Edit { auto_start, .. } => assert_eq!(auto_start, Some(false)),
        other => panic!("expected edit command, got {other:?}"),
    }
}

#[test]
fn td_c_08_parse_edit_auto_start_true_without_value() {
    let cli = Cli::try_parse_from(["scmux", "edit", "alpha", "--auto-start"])
        .expect("parse edit auto-start true");

    match cli.command {
        Command::Edit { auto_start, .. } => assert_eq!(auto_start, Some(true)),
        other => panic!("expected edit command, got {other:?}"),
    }
}

#[test]
fn td_c_09_parse_doctor_command() {
    let cli = Cli::try_parse_from(["scmux", "doctor"]).expect("parse doctor command");
    match cli.command {
        Command::Doctor => {}
        other => panic!("expected doctor command, got {other:?}"),
    }
}

#[test]
fn td_c_10_parse_session_add_command() {
    let cli = Cli::try_parse_from([
        "scmux",
        "session",
        "add",
        "--name",
        "alpha",
        "--project",
        "demo",
        "--config",
        "alpha.json",
        "--auto-start",
    ])
    .expect("parse session add command");

    match cli.command {
        Command::Session { command } => match command {
            SessionCommand::Add {
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
            other => panic!("expected session add command, got {other:?}"),
        },
        other => panic!("expected session command, got {other:?}"),
    }
}

#[test]
fn td_c_11_parse_session_edit_auto_start_true_without_value() {
    let cli = Cli::try_parse_from(["scmux", "session", "edit", "alpha", "--auto-start"])
        .expect("parse session edit auto-start true");

    match cli.command {
        Command::Session { command } => match command {
            SessionCommand::Edit { auto_start, .. } => assert_eq!(auto_start, Some(true)),
            other => panic!("expected session edit command, got {other:?}"),
        },
        other => panic!("expected session command, got {other:?}"),
    }
}

#[test]
fn td_c_12_parse_host_add_command() {
    let cli = Cli::try_parse_from([
        "scmux",
        "host",
        "add",
        "--name",
        "local",
        "--address",
        "127.0.0.1",
        "--ssh-user",
        "dev",
        "--api-port",
        "9000",
    ])
    .expect("parse host add command");

    match cli.command {
        Command::Host { command } => match command {
            HostCommand::Add {
                name,
                address,
                ssh_user,
                api_port,
                ..
            } => {
                assert_eq!(name, "local");
                assert_eq!(address, "127.0.0.1");
                assert_eq!(ssh_user.as_deref(), Some("dev"));
                assert_eq!(api_port, Some(9000));
            }
            other => panic!("expected host add command, got {other:?}"),
        },
        other => panic!("expected host command, got {other:?}"),
    }
}

#[test]
fn td_c_13_parse_host_edit_clear_ssh_user() {
    let cli = Cli::try_parse_from(["scmux", "host", "edit", "12", "--clear-ssh-user"])
        .expect("parse host edit clear ssh-user command");

    match cli.command {
        Command::Host { command } => match command {
            HostCommand::Edit {
                id,
                clear_ssh_user,
                ssh_user,
                ..
            } => {
                assert_eq!(id, 12);
                assert!(clear_ssh_user);
                assert!(ssh_user.is_none());
            }
            other => panic!("expected host edit command, got {other:?}"),
        },
        other => panic!("expected host command, got {other:?}"),
    }
}
