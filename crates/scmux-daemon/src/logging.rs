use std::sync::OnceLock;

static INIT: OnceLock<()> = OnceLock::new();

pub(crate) fn parse_level_from(val: Option<&str>) -> tracing::Level {
    match val.unwrap_or("info").to_ascii_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

pub(crate) fn parse_level() -> tracing::Level {
    let value = std::env::var("SCMUX_LOG").ok();
    parse_level_from(value.as_deref())
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
    // TODO: size-based rotation (50 MiB / 5 files) requires a custom appender or
    // tracing-appender extension. Current implementation writes JSONL to one file.
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

pub fn init_logging(
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
    _source_binary: &'static str,
    file_path: std::path::PathBuf,
    rotation: RotationConfig,
) -> anyhow::Result<LoggingGuards> {
    if INIT.get().is_some() {
        return Ok(LoggingGuards::empty());
    }

    ensure_log_dir(&file_path)?;
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scmux-daemon.log");
    let dir = file_path.parent().unwrap_or(std::path::Path::new("."));

    let file_appender = tracing_appender::rolling::never(dir, file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .json()
        .with_writer(non_blocking)
        .with_max_level(parse_level())
        .try_init()
        .ok();

    tracing::debug!(
        path = %file_path.display(),
        max_bytes = rotation.max_bytes,
        max_files = rotation.max_files,
        "daemon writer logging initialized"
    );
    let _ = INIT.set(());
    Ok(LoggingGuards {
        _guards: vec![Box::new(guard)],
    })
}

pub(crate) fn ensure_log_dir(file_path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-D-14: ensure_log_dir() creates the parent directory for the log file.
    #[test]
    fn td_14_ensure_log_dir_creates_parent_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("nested/scmux-daemon.log");
        ensure_log_dir(&log_path).expect("ensure log dir");
        assert!(log_path.parent().expect("parent").exists());
    }

    /// T-D-15: parse_level_from() resolves warn correctly.
    #[test]
    fn td_15_parse_level_returns_warn() {
        assert_eq!(parse_level_from(Some("warn")), tracing::Level::WARN);
    }

    /// T-D-16: parse_level_from() resolves debug correctly.
    /// This covers the --verbose path because main.rs resolves verbose to "debug".
    #[test]
    fn td_16_parse_level_returns_debug() {
        assert_eq!(parse_level_from(Some("debug")), tracing::Level::DEBUG);
    }
}
