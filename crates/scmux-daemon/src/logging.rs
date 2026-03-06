use std::sync::OnceLock;

static INIT: OnceLock<()> = OnceLock::new();

pub(crate) fn parse_level() -> tracing::Level {
    match std::env::var("SCMUX_LOG")
        .unwrap_or_else(|_| "info".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

fn _init_stderr() {
    if INIT.get().is_some() {
        return;
    }
    let level = parse_level();
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .with_target(false)
        .try_init();
    let _ = INIT.set(());
}

#[derive(Debug, Clone)]
pub struct RotationConfig {
    pub max_bytes: u64,
    pub max_files: u32,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024,
            max_files: 5,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum UnifiedLogMode {
    DaemonWriter {
        file_path: std::path::PathBuf,
        rotation: RotationConfig,
    },
    StderrOnly,
}

pub struct LoggingGuards {
    _guards: Vec<Box<dyn std::any::Any + Send>>,
}

impl LoggingGuards {
    fn empty() -> Self {
        Self {
            _guards: Vec::new(),
        }
    }
}

pub fn init_unified(
    source_binary: &'static str,
    mode: UnifiedLogMode,
) -> anyhow::Result<LoggingGuards> {
    match mode {
        UnifiedLogMode::StderrOnly => {
            _init_stderr();
            Ok(LoggingGuards::empty())
        }
        UnifiedLogMode::DaemonWriter {
            file_path,
            rotation,
        } => setup_daemon_writer(source_binary, file_path, rotation),
    }
}

pub fn init_stderr_only() -> LoggingGuards {
    _init_stderr();
    LoggingGuards::empty()
}

fn setup_daemon_writer(
    source_binary: &'static str,
    file_path: std::path::PathBuf,
    rotation: RotationConfig,
) -> anyhow::Result<LoggingGuards> {
    if INIT.get().is_some() {
        return Ok(LoggingGuards::empty());
    }

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)?;

    let level = parse_level();
    let writer_path = file_path.clone();
    tracing_subscriber::fmt()
        .with_writer(move || -> Box<dyn std::io::Write + Send> {
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&writer_path)
            {
                Ok(file) => Box::new(file),
                Err(_) => Box::new(std::io::stderr()),
            }
        })
        .with_ansi(false)
        .with_max_level(level)
        .with_target(false)
        .try_init()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    tracing::debug!(
        source_binary,
        path = %file_path.display(),
        max_bytes = rotation.max_bytes,
        max_files = rotation.max_files,
        "DaemonWriter logging initialized"
    );
    let _ = INIT.set(());
    Ok(LoggingGuards::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-D-14: init_unified() with DaemonWriter mode creates the parent directory for the log file.
    /// Uses a tempdir so this test is side-effect-free and does not touch ~/.config/scmux/.
    #[test]
    fn td_14_daemon_writer_creates_log_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("subdir").join("scmux-daemon.log");

        // The subdirectory does not exist yet — init_unified must create it.
        assert!(!log_path.parent().unwrap().exists());

        // init_unified may fail if a global subscriber is already installed (other tests),
        // but the directory creation happens before subscriber registration, so we only
        // check the filesystem outcome.
        let _ = init_unified(
            "test",
            UnifiedLogMode::DaemonWriter {
                file_path: log_path.clone(),
                rotation: RotationConfig::default(),
            },
        );

        assert!(
            log_path.parent().unwrap().exists(),
            "parent directory was not created by init_unified DaemonWriter"
        );
    }

    /// T-D-15: parse_level() returns Level::WARN when SCMUX_LOG=warn.
    #[test]
    fn td_15_parse_level_returns_warn_for_scmux_log_warn() {
        // Use a sub-process-safe approach: set env var, call parse_level, restore.
        // This is safe in a single-threaded test context.
        let prev = std::env::var("SCMUX_LOG").ok();
        std::env::set_var("SCMUX_LOG", "warn");
        let level = parse_level();
        match prev {
            Some(v) => std::env::set_var("SCMUX_LOG", v),
            None => std::env::remove_var("SCMUX_LOG"),
        }
        assert_eq!(level, tracing::Level::WARN);
    }

    /// T-D-16: parse_level() returns Level::DEBUG when SCMUX_LOG=debug.
    /// This also covers the --verbose flag path: main.rs sets SCMUX_LOG=debug when --verbose
    /// is passed, and parse_level() is called immediately after.
    #[test]
    fn td_16_parse_level_returns_debug_for_scmux_log_debug() {
        let prev = std::env::var("SCMUX_LOG").ok();
        std::env::set_var("SCMUX_LOG", "debug");
        let level = parse_level();
        match prev {
            Some(v) => std::env::set_var("SCMUX_LOG", v),
            None => std::env::remove_var("SCMUX_LOG"),
        }
        assert_eq!(level, tracing::Level::DEBUG);
    }
}
