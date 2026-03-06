use std::sync::OnceLock;

static INIT: OnceLock<()> = OnceLock::new();

fn parse_level() -> tracing::Level {
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
