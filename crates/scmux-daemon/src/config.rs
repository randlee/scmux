use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub log_level: Option<String>,
    pub port: Option<u16>,
    pub db_path: Option<String>,
    pub poll_interval_secs: Option<u64>,
    #[serde(default)]
    pub remote_hosts: Vec<RemoteHost>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoteHost {
    pub name: String,
    pub hostname: String,
    pub ssh_user: Option<String>,
}

/// Loads config from TOML. Missing file is not an error and returns defaults.
///
/// Example:
/// ```toml
/// log_level = "info"
/// port = 7700
/// db_path = "~/.config/scmux/scmux.db"
/// poll_interval_secs = 15
///
/// [[remote_hosts]]
/// name = "dgx-spark"
/// hostname = "192.168.1.50"
/// ssh_user = "randlee"
/// ```
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let cfg = toml::from_str::<Config>(&raw)?;
    Ok(cfg)
}
