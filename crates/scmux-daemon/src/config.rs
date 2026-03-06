use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub polling: PollingConfig,
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

#[derive(Debug, Deserialize, Clone)]
pub struct HostConfig {
    pub name: String,
    pub address: String,
    pub ssh_user: Option<String>,
    pub api_port: Option<u16>,
    pub is_local: Option<bool>,
}

/// Loads config from TOML. Missing file is not an error and returns defaults.
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let cfg = toml::from_str::<Config>(&raw)?;
    Ok(cfg)
}
