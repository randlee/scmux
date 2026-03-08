use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub polling: PollingConfig,
    pub atm: AtmConfig,
    pub hosts: Vec<HostConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DaemonConfig {
    pub port: Option<u16>,
    pub db_path: Option<String>,
    pub default_terminal: Option<String>,
    pub log_level: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PollingConfig {
    pub tmux_interval_secs: Option<u64>,
    pub health_interval_secs: Option<u64>,
    pub ci_active_interval_secs: Option<u64>,
    pub ci_idle_interval_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AtmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub teams: Vec<String>,
    #[serde(default)]
    pub allow_shutdown: bool,
    pub socket_path: Option<String>,
    pub stuck_minutes: Option<u64>,
    pub stop_grace_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HostConfig {
    pub name: String,
    pub address: String,
    pub ssh_user: Option<String>,
    pub api_port: Option<u16>,
    pub is_local: Option<bool>,
}

/// Loads config from TOML. Missing file is not an error and returns defaults.
pub fn load_config(path: Option<&Path>) -> anyhow::Result<Config> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let cfg = toml::from_str::<Config>(&raw)?;
    Ok(cfg)
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = std::env::var("SCMUX_CONFIG")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| std::path::PathBuf::from(h).join(".config/scmux/scmux.toml"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("scmux.toml"));
        load_config(Some(&config_path))
    }
}
