//! Typed HTTP client for `scmux-daemon`.

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Default daemon endpoint for CLI requests (canonical daemon port 7878).
pub const DEFAULT_DAEMON_URL: &str = "http://localhost:7878";

#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug)]
pub enum ClientError {
    ConnectionRefused,
    NotFound,
    HttpStatus(u16, String),
    Transport(String),
    Decode(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionRefused => {
                write!(f, "scmux: daemon not running (start with scmux-daemon)")
            }
            Self::NotFound => write!(f, "scmux: resource not found"),
            Self::HttpStatus(code, body) => {
                if body.trim().is_empty() {
                    write!(f, "scmux: daemon request failed ({code})")
                } else {
                    write!(f, "scmux: daemon request failed ({code}): {body}")
                }
            }
            Self::Transport(message) => write!(f, "scmux: request failed: {message}"),
            Self::Decode(message) => write!(f, "scmux: invalid daemon response: {message}"),
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaneSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_activity: Option<String>,
    #[serde(default)]
    pub current_command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionCiSummary {
    pub provider: String,
    pub status: String,
    #[serde(default)]
    pub data_json: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_message: Option<String>,
    #[serde(default)]
    pub polled_at: Option<String>,
    #[serde(default)]
    pub next_poll_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionAtmSummary {
    pub state: String,
    #[serde(default)]
    pub last_transition: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub project: Option<String>,
    pub host_id: i64,
    pub status: String,
    #[serde(default)]
    pub cron_schedule: Option<String>,
    pub auto_start: bool,
    #[serde(default)]
    pub panes: Vec<PaneSummary>,
    #[serde(default)]
    pub polled_at: Option<String>,
    #[serde(default)]
    pub session_ci: Vec<SessionCiSummary>,
    #[serde(default)]
    pub atm: Option<SessionAtmSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionDetail {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub project: Option<String>,
    pub host_id: i64,
    pub status: String,
    #[serde(default)]
    pub cron_schedule: Option<String>,
    pub auto_start: bool,
    pub panes: serde_json::Value,
    #[serde(default)]
    pub polled_at: Option<String>,
    #[serde(default)]
    pub session_ci: Vec<SessionCiSummary>,
    #[serde(default)]
    pub atm: Option<SessionAtmSummary>,
    pub config_json: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostSummary {
    pub id: i64,
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub ssh_user: Option<String>,
    pub api_port: u16,
    pub is_local: bool,
    #[serde(default)]
    pub last_seen: Option<String>,
    pub reachable: bool,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub session_count: i64,
    pub db_path: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub config_json: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_project: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PatchSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_json: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_schedule: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_repo: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_project: Option<Option<String>>,
}

impl PatchSessionRequest {
    pub fn is_empty(&self) -> bool {
        self.project.is_none()
            && self.config_json.is_none()
            && self.cron_schedule.is_none()
            && self.auto_start.is_none()
            && self.enabled.is_none()
            && self.github_repo.is_none()
            && self.azure_project.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JumpRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<i64>,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: normalize_host(&base_url),
            http: reqwest::Client::new(),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, ClientError> {
        self.request_json(Method::GET, "/sessions", None::<&()>)
            .await
    }

    pub async fn get_session(&self, name: &str) -> Result<SessionDetail, ClientError> {
        self.request_json(Method::GET, &format!("/sessions/{name}"), None::<&()>)
            .await
    }

    pub async fn start_session(&self, name: &str) -> Result<ActionResponse, ClientError> {
        self.request_json(
            Method::POST,
            &format!("/sessions/{name}/start"),
            None::<&()>,
        )
        .await
    }

    pub async fn stop_session(&self, name: &str) -> Result<ActionResponse, ClientError> {
        self.request_json(Method::POST, &format!("/sessions/{name}/stop"), None::<&()>)
            .await
    }

    pub async fn jump_session(
        &self,
        name: &str,
        req: &JumpRequest,
    ) -> Result<ActionResponse, ClientError> {
        self.request_json(Method::POST, &format!("/sessions/{name}/jump"), Some(req))
            .await
    }

    pub async fn create_session(
        &self,
        req: &CreateSessionRequest,
    ) -> Result<ActionResponse, ClientError> {
        self.request_json(Method::POST, "/sessions", Some(req))
            .await
    }

    pub async fn patch_session(
        &self,
        name: &str,
        req: &PatchSessionRequest,
    ) -> Result<ActionResponse, ClientError> {
        self.request_json(Method::PATCH, &format!("/sessions/{name}"), Some(req))
            .await
    }

    pub async fn delete_session(&self, name: &str) -> Result<ActionResponse, ClientError> {
        self.request_json(Method::DELETE, &format!("/sessions/{name}"), None::<&()>)
            .await
    }

    pub async fn list_hosts(&self) -> Result<Vec<HostSummary>, ClientError> {
        self.request_json(Method::GET, "/hosts", None::<&()>).await
    }

    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        self.request_json(Method::GET, "/health", None::<&()>).await
    }

    async fn request_json<T, B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ClientError>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut req = self.http.request(method, url);
        if let Some(payload) = body {
            req = req.json(payload);
        }

        let response = req.send().await.map_err(map_send_error)?;
        let status = response.status();

        if status == StatusCode::NOT_FOUND {
            return Err(ClientError::NotFound);
        }

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(ClientError::HttpStatus(status.as_u16(), body_text));
        }

        response
            .json::<T>()
            .await
            .map_err(|err| ClientError::Decode(err.to_string()))
    }
}

pub fn resolve_base_url(host_flag: Option<&str>) -> String {
    if let Some(flag_host) = host_flag {
        if !flag_host.trim().is_empty() {
            return normalize_host(flag_host);
        }
    }

    if let Ok(env_host) = std::env::var("SCMUX_HOST") {
        if !env_host.trim().is_empty() {
            return normalize_host(&env_host);
        }
    }

    DEFAULT_DAEMON_URL.to_string()
}

fn normalize_host(raw: &str) -> String {
    let value = raw.trim().trim_end_matches('/');
    if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    }
}

fn map_send_error(err: reqwest::Error) -> ClientError {
    if err.is_connect() {
        ClientError::ConnectionRefused
    } else {
        ClientError::Transport(err.to_string())
    }
}
