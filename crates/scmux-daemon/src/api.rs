use axum::{
    extract::{Path, Request, State},
    http::{header, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::atm::ShutdownTarget;
use crate::runtime::{AtmRuntimeSummary, CiRuntimeSummary};
use crate::tmux::{self, HostTarget};
use crate::{atm, db, definition_writer, AppState};

pub fn router(state: Arc<AppState>) -> Router {
    let middleware_state = Arc::clone(&state);
    Router::new()
        .route("/", get(dashboard_index))
        .route("/dashboard.js", get(dashboard_js))
        .route("/react.min.js", get(react_js))
        .route("/react-dom.min.js", get(react_dom_js))
        .route("/health", get(health))
        .route("/hosts", get(list_hosts).post(create_host))
        .route(
            "/hosts/:id",
            axum::routing::patch(patch_host).delete(delete_host),
        )
        .route("/dashboard-config.json", get(get_dashboard_config))
        .route("/discovery", get(get_discovery))
        .route("/sessions", get(list_sessions).post(create_session))
        .route(
            "/sessions/:name",
            get(get_session).patch(patch_session).delete(delete_session),
        )
        .route("/sessions/:name/start", post(start_session))
        .route("/sessions/:name/stop", post(stop_session))
        .route("/sessions/:name/jump", post(jump_session))
        .layer(CorsLayer::permissive())
        .layer(from_fn_with_state(middleware_state, touch_last_api_access))
        .with_state(state)
}

const DASHBOARD_HTML: &[u8] = include_bytes!("../assets/index.html");
const DASHBOARD_JS: &[u8] = include_bytes!("../assets/dashboard.js");
const REACT_JS: &[u8] = include_bytes!("../assets/react.min.js");
const REACT_DOM_JS: &[u8] = include_bytes!("../assets/react-dom.min.js");
const DEFAULT_POLL_INTERVAL_SECS: u64 = 15;
const DEFAULT_STOP_GRACE_SECS: u64 = 10;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    uptime_secs: u64,
    session_count: i64,
    sessions_running: i64,
    host_id: i64,
    atm_available: bool,
    ci_available: CiAvailability,
    pollers: PollerStates,
    recent_errors: Vec<String>,
    db_path: String,
    version: &'static str,
}

#[derive(Serialize)]
struct CiAvailability {
    gh: bool,
    az: bool,
}

#[derive(Serialize)]
struct PollerStates {
    tmux: crate::PollerHealth,
    hosts: crate::PollerHealth,
    ci: crate::PollerHealth,
    atm: crate::PollerHealth,
}

#[derive(Serialize)]
struct SessionSummary {
    id: i64,
    name: String,
    project: Option<String>,
    host_id: i64,
    status: String,
    cron_schedule: Option<String>,
    auto_start: bool,
    panes: serde_json::Value,
    polled_at: Option<String>,
    session_ci: Vec<SessionCiSummary>,
    atm: Option<SessionAtmSummary>,
}

#[derive(Serialize, Clone)]
struct SessionCiSummary {
    provider: String,
    status: String,
    data_json: Option<serde_json::Value>,
    tool_message: Option<String>,
    polled_at: Option<String>,
    next_poll_at: Option<String>,
}

#[derive(Serialize, Clone)]
struct SessionAtmSummary {
    state: String,
    last_transition: Option<String>,
}

#[derive(Serialize)]
struct SessionDetail {
    #[serde(flatten)]
    summary: SessionSummary,
    config_json: serde_json::Value,
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    ok: bool,
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    name: String,
    project: Option<String>,
    host_id: Option<i64>,
    config_json: serde_json::Value,
    cron_schedule: Option<String>,
    auto_start: Option<bool>,
    github_repo: Option<String>,
    azure_project: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PatchSessionRequest {
    project: Option<Option<String>>,
    config_json: Option<serde_json::Value>,
    cron_schedule: Option<Option<String>>,
    auto_start: Option<bool>,
    enabled: Option<bool>,
    github_repo: Option<Option<String>>,
    azure_project: Option<Option<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct JumpRequest {
    terminal: Option<String>,
    host_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateHostRequest {
    name: String,
    address: String,
    ssh_user: Option<String>,
    api_port: Option<u16>,
    is_local: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct PatchHostRequest {
    name: Option<String>,
    address: Option<String>,
    ssh_user: Option<Option<String>>,
    api_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
struct HostSummary {
    id: i64,
    name: String,
    address: String,
    ssh_user: Option<String>,
    api_port: u16,
    is_local: bool,
    last_seen: Option<String>,
    reachable: bool,
    url: String,
}

#[derive(Debug, Serialize)]
struct DashboardConfigResponse {
    hosts: Vec<HostSummary>,
    default_terminal: String,
    poll_interval_ms: u64,
}

async fn serve_dashboard_asset(
    path: &str,
    embedded_bytes: &'static [u8],
    content_type: &'static str,
) -> Response {
    if let Ok(dir) = std::env::var("SCMUX_DASHBOARD_DIR") {
        let file_path = PathBuf::from(dir).join(path);
        return match tokio::fs::read(file_path).await {
            Ok(bytes) => ([(header::CONTENT_TYPE, content_type)], bytes).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        };
    }

    ([(header::CONTENT_TYPE, content_type)], embedded_bytes).into_response()
}

async fn dashboard_index() -> Response {
    serve_dashboard_asset("index.html", DASHBOARD_HTML, "text/html; charset=utf-8").await
}

async fn dashboard_js() -> Response {
    serve_dashboard_asset(
        "dashboard.js",
        DASHBOARD_JS,
        "application/javascript; charset=utf-8",
    )
    .await
}

async fn react_js() -> Response {
    serve_dashboard_asset(
        "react.min.js",
        REACT_JS,
        "application/javascript; charset=utf-8",
    )
    .await
}

async fn react_dom_js() -> Response {
    serve_dashboard_asset(
        "react-dom.min.js",
        REACT_DOM_JS,
        "application/javascript; charset=utf-8",
    )
    .await
}

async fn touch_last_api_access(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    state.mark_api_access();
    next.run(request).await
}

async fn health(State(state): State<Arc<AppState>>) -> Result<Json<HealthResponse>, StatusCode> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    let db_path = state.db_path.clone();
    let host_id = state.host_id;
    let atm_available = state.atm_available.load(Ordering::Relaxed);
    let ci_available = CiAvailability {
        gh: state.ci_tools.gh_available,
        az: state.ci_tools.az_available,
    };
    let sessions_running = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.live_session_count()
    };
    let health_state = state.runtime_health();

    let session_count_task = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().expect("db lock");
        db.query_row("SELECT COUNT(*) FROM sessions WHERE enabled = 1", [], |r| {
            r.get(0)
        })
    });

    let session_count: i64 = match session_count_task.await {
        Ok(Ok(value)) => value,
        Ok(Err(err)) => {
            tracing::warn!("health query failed: {err}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(err) => {
            tracing::warn!("health join error: {err}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(HealthResponse {
        status: "ok",
        uptime_secs,
        session_count,
        sessions_running,
        host_id,
        atm_available,
        ci_available,
        pollers: PollerStates {
            tmux: health_state.tmux,
            hosts: health_state.hosts,
            ci: health_state.ci,
            atm: health_state.atm,
        },
        recent_errors: health_state.recent_errors,
        db_path,
        version: env!("CARGO_PKG_VERSION"),
    }))
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SessionSummary>>, StatusCode> {
    let session_rows = {
        let state = Arc::clone(&state);
        let joined = tokio::task::spawn_blocking(move || {
            let db = state.db.lock().expect("db lock");
            db::list_sessions_for_host(&db, state.host_id)
        })
        .await;

        match joined {
            Ok(Ok(rows)) => rows,
            Ok(Err(err)) => {
                tracing::warn!("list_sessions DB error: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            Err(err) => {
                tracing::warn!("list_sessions join error: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

    let atm_available = state.atm_available.load(Ordering::Relaxed);
    let sessions = {
        let runtime = state.runtime.lock().expect("runtime lock");
        session_rows
            .into_iter()
            .map(|row| {
                let runtime_row = runtime.session(&row.name).cloned().unwrap_or_default();
                let ci = runtime
                    .ci_for_session(&row.name)
                    .into_iter()
                    .map(from_ci_runtime)
                    .collect::<Vec<_>>();
                let atm = if atm_available {
                    runtime.atm_for_session(&row.name).map(from_atm_runtime)
                } else {
                    None
                };

                SessionSummary {
                    id: row.id,
                    name: row.name,
                    project: row.project,
                    host_id: row.host_id,
                    status: runtime_row.status,
                    cron_schedule: row.cron_schedule,
                    auto_start: row.auto_start,
                    panes: serde_json::to_value(runtime_row.panes).unwrap_or(serde_json::json!([])),
                    polled_at: runtime_row.polled_at,
                    session_ci: ci,
                    atm,
                }
            })
            .collect::<Vec<_>>()
    };

    Ok(Json(sessions))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SessionDetail>, StatusCode> {
    let row = {
        let state = Arc::clone(&state);
        let name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let db = state.db.lock().expect("db lock");
            db::get_session_for_host(&db, state.host_id, &name)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        result.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }
    .ok_or(StatusCode::NOT_FOUND)?;

    let config_json = serde_json::from_str(&row.config_json).unwrap_or(serde_json::json!({}));
    let atm_available = state.atm_available.load(Ordering::Relaxed);
    let (runtime_row, ci, atm) = {
        let runtime = state.runtime.lock().expect("runtime lock");
        let runtime_row = runtime.session(&row.name).cloned().unwrap_or_default();
        let ci = runtime
            .ci_for_session(&row.name)
            .into_iter()
            .map(from_ci_runtime)
            .collect::<Vec<_>>();
        let atm = if atm_available {
            runtime.atm_for_session(&row.name).map(from_atm_runtime)
        } else {
            None
        };
        (runtime_row, ci, atm)
    };

    Ok(Json(SessionDetail {
        summary: SessionSummary {
            id: row.id,
            name: row.name,
            project: row.project,
            host_id: row.host_id,
            status: runtime_row.status,
            cron_schedule: row.cron_schedule,
            auto_start: row.auto_start,
            panes: serde_json::to_value(runtime_row.panes).unwrap_or(serde_json::json!([])),
            polled_at: runtime_row.polled_at,
            session_ci: ci,
            atm,
        },
        config_json,
    }))
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let host_id = req.host_id.unwrap_or(state.host_id);
    let config_json = serde_json::to_string(&req.config_json).map_err(|_| {
        bad_request(
            "invalid_json",
            "config_json payload could not be serialized".to_string(),
        )
    })?;

    let new_session = db::NewSession {
        name: req.name.clone(),
        project: req.project,
        host_id,
        config_json,
        cron_schedule: req.cron_schedule,
        auto_start: req.auto_start.unwrap_or(false),
        github_repo: req.github_repo,
        azure_project: req.azure_project,
    };

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_session(&db_conn, &new_session)
    })
    .await
    .map_err(|_| internal_error("failed to join create_session task".to_string()))?;

    match result {
        Ok(_) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{}' created", req.name),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<PatchSessionRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response_name = name.clone();
    let patch = db::SessionPatch {
        project: req.project,
        config_json: req
            .config_json
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok()),
        cron_schedule: req.cron_schedule,
        auto_start: req.auto_start,
        enabled: req.enabled,
        github_repo: req.github_repo,
        azure_project: req.azure_project,
    };

    let host_id = state.host_id;
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_session(&db_conn, host_id, &name, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_session task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{response_name}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("session '{response_name}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response_name = name.clone();
    let host_id = state.host_id;
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::delete_session(&db_conn, host_id, &name)
    })
    .await
    .map_err(|_| internal_error("failed to join delete_session task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{response_name}' disabled"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("session '{response_name}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn start_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let definition = {
        let state = Arc::clone(&state);
        let name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let db_conn = state.db.lock().expect("db lock");
            db::get_session_for_host(&db_conn, state.host_id, &name)
        })
        .await
        .map_err(|_| internal_error("failed to join start_session read task".to_string()))?;
        result.map_err(|_| internal_error("failed to read session definition".to_string()))?
    }
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("session '{name}' not found"),
            }),
        )
    })?;

    if let Err(message) = validate_start_config(&definition.config_json, &name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                ok: false,
                code: "invalid_config".to_string(),
                message,
            }),
        ));
    }

    {
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.mark_starting(&name);
    }

    let action = tmux::start_session(&name, &definition.config_json).await;
    match action {
        Ok(()) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{name}' started"),
        })),
        Err(err) => {
            let mut runtime = state.runtime.lock().expect("runtime lock");
            runtime.mark_start_failed(&name, err.to_string());
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    ok: false,
                    code: "start_failed".to_string(),
                    message: err.to_string(),
                }),
            ))
        }
    }
}

async fn stop_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let definition = {
        let state = Arc::clone(&state);
        let name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let db_conn = state.db.lock().expect("db lock");
            db::get_session_for_host(&db_conn, state.host_id, &name)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        result.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }
    .ok_or(StatusCode::NOT_FOUND)?;

    let targets = extract_shutdown_targets(&definition.config_json);
    if let Err(err) = atm::send_shutdown_messages(state.as_ref(), &targets).await {
        tracing::warn!("atm shutdown send failed for session '{name}': {err}");
    }

    let grace_secs = state
        .config
        .atm
        .stop_grace_secs
        .unwrap_or(DEFAULT_STOP_GRACE_SECS)
        .max(1);
    tokio::time::sleep(tokio::time::Duration::from_secs(grace_secs)).await;

    let live = tmux::live_sessions().await.unwrap_or_default();
    let still_running = live.contains_key(&name);

    let response = if still_running {
        match tmux::stop_session(&name).await {
            Ok(()) => ActionResponse {
                ok: true,
                message: format!("session '{name}' stopped after graceful timeout"),
            },
            Err(err) => ActionResponse {
                ok: false,
                message: err.to_string(),
            },
        }
    } else {
        ActionResponse {
            ok: true,
            message: format!("session '{name}' stopped gracefully"),
        }
    };

    if response.ok {
        let mut runtime = state.runtime.lock().expect("runtime lock");
        runtime.mark_stopped(&name);
    }

    Ok(Json(response))
}

async fn jump_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<JumpRequest>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let host_id = req.host_id.unwrap_or(state.host_id);
    let state2 = Arc::clone(&state);
    let name2 = name.clone();
    let session_data = tokio::task::spawn_blocking(move || {
        let db_conn = state2.db.lock().expect("db lock");
        let session = db::get_session_for_host(&db_conn, host_id, &name2)?;
        let host = db::get_host(&db_conn, host_id)?;
        Ok::<_, anyhow::Error>((session, host))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (_session, host) = session_data;
    let host = host.ok_or(StatusCode::NOT_FOUND)?;

    let terminal = req
        .terminal
        .or_else(|| state.config.daemon.default_terminal.clone())
        .unwrap_or_else(|| "iterm2".to_string());

    let target = if host.is_local {
        HostTarget::Local
    } else {
        HostTarget::Remote {
            user: host.ssh_user.ok_or(StatusCode::BAD_REQUEST)?,
            host: host.address,
        }
    };

    let result = tmux::jump_session(target, &name, &terminal).await;
    match result {
        Ok(message) => Ok(Json(ActionResponse { ok: true, message })),
        Err(err) => Ok(Json(ActionResponse {
            ok: false,
            message: err.to_string(),
        })),
    }
}

async fn list_hosts(State(state): State<Arc<AppState>>) -> Json<Vec<HostSummary>> {
    let hosts = fetch_hosts(state).await.unwrap_or_default();
    Json(hosts)
}

async fn create_host(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateHostRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let host = db::NewHost {
        name: req.name.clone(),
        address: req.address,
        ssh_user: req.ssh_user,
        api_port: req.api_port.unwrap_or(7878),
        is_local: req.is_local.unwrap_or(false),
    };

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_host(&db_conn, &host)
    })
    .await
    .map_err(|_| internal_error("failed to join create_host task".to_string()))?;

    match result {
        Ok(_) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("host '{}' created", req.name),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<PatchHostRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let patch = db::HostPatch {
        name: req.name,
        address: req.address,
        ssh_user: req.ssh_user,
        api_port: req.api_port,
    };

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_host(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_host task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("host '{id}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("host '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn delete_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::delete_host(&db_conn, id)
    })
    .await
    .map_err(|_| internal_error("failed to join delete_host task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("host '{id}' disabled"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("host '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn get_dashboard_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DashboardConfigResponse>, StatusCode> {
    let hosts = fetch_hosts(Arc::clone(&state))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(DashboardConfigResponse {
        hosts,
        default_terminal: state
            .config
            .daemon
            .default_terminal
            .clone()
            .unwrap_or_else(|| "iterm2".to_string()),
        poll_interval_ms: state
            .config
            .polling
            .tmux_interval_secs
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECS)
            * 1_000,
    }))
}

async fn get_discovery(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<crate::runtime::DiscoverySession>> {
    let rows = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.discovery_rows()
    };
    Json(rows)
}

async fn fetch_hosts(state: Arc<AppState>) -> anyhow::Result<Vec<HostSummary>> {
    let reachability = {
        let map = state.reachability.lock().expect("reachability lock");
        map.clone()
    };

    let rows = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        db::list_hosts(&db_conn)
    })
    .await??;

    let hosts = rows
        .into_iter()
        .map(|row| {
            let reach = reachability.get(&row.id);
            let reachable = if row.is_local {
                true
            } else {
                reach.map(|entry| entry.reachable).unwrap_or(false)
            };
            let last_seen = reach
                .and_then(|entry| entry.last_seen.clone())
                .or(row.last_seen);
            HostSummary {
                id: row.id,
                name: row.name,
                address: row.address.clone(),
                ssh_user: row.ssh_user,
                api_port: row.api_port,
                is_local: row.is_local,
                last_seen,
                reachable,
                url: format!("http://{}:{}", row.address, row.api_port),
            }
        })
        .collect::<Vec<_>>();

    Ok(hosts)
}

fn from_ci_runtime(entry: CiRuntimeSummary) -> SessionCiSummary {
    SessionCiSummary {
        provider: entry.provider,
        status: entry.status,
        data_json: entry.data_json,
        tool_message: entry.tool_message,
        polled_at: entry.polled_at,
        next_poll_at: entry.next_poll_at,
    }
}

fn from_atm_runtime(entry: AtmRuntimeSummary) -> SessionAtmSummary {
    SessionAtmSummary {
        state: entry.state,
        last_transition: entry.last_transition,
    }
}

fn validate_start_config(config_json: &str, name: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|err| format!("config_json is not valid JSON: {err}"))?;
    let session_name = value
        .get("session_name")
        .and_then(|raw| raw.as_str())
        .ok_or_else(|| "config_json.session_name is required".to_string())?;
    if session_name != name {
        return Err("config_json.session_name must equal route session name".to_string());
    }
    let panes = value
        .get("panes")
        .and_then(|raw| raw.as_array())
        .ok_or_else(|| "config_json.panes[] is required".to_string())?;
    if panes.is_empty() {
        return Err("config_json.panes[] must contain at least one pane".to_string());
    }
    Ok(())
}

fn extract_shutdown_targets(config_json: &str) -> Vec<ShutdownTarget> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(config_json) else {
        return Vec::new();
    };
    let Some(panes) = value.get("panes").and_then(|raw| raw.as_array()) else {
        return Vec::new();
    };

    panes
        .iter()
        .filter_map(|pane| {
            let team = pane.get("atm_team").and_then(|raw| raw.as_str())?.trim();
            let agent = pane.get("atm_agent").and_then(|raw| raw.as_str())?.trim();
            if team.is_empty() || agent.is_empty() {
                return None;
            }
            Some(ShutdownTarget {
                team: team.to_string(),
                agent: agent.to_string(),
            })
        })
        .collect::<Vec<_>>()
}

fn map_write_error(err: definition_writer::WriteError) -> (StatusCode, Json<ErrorResponse>) {
    match err {
        definition_writer::WriteError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: "resource not found".to_string(),
            }),
        ),
        definition_writer::WriteError::Conflict(message) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                ok: false,
                code: "conflict".to_string(),
                message,
            }),
        ),
        definition_writer::WriteError::Validation(message) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                ok: false,
                code: "validation_error".to_string(),
                message,
            }),
        ),
        definition_writer::WriteError::Forbidden(message) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                ok: false,
                code: "forbidden".to_string(),
                message,
            }),
        ),
        definition_writer::WriteError::Internal(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                ok: false,
                code: "internal_error".to_string(),
                message,
            }),
        ),
    }
}

fn bad_request(code: &str, message: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            ok: false,
            code: code.to_string(),
            message,
        }),
    )
}

fn internal_error(message: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            ok: false,
            code: "internal_error".to_string(),
            message,
        }),
    )
}
