use axum::{
    extract::{Path, Request, State},
    http::{header, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path as FsPath, PathBuf};
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
        .route("/runtime/crews", get(list_runtime_crews))
        .route(
            "/runtime/discovery/unregistered",
            get(get_unregistered_discovery),
        )
        .route("/sessions", get(list_sessions).post(create_session))
        .route(
            "/sessions/:name",
            get(get_session).patch(patch_session).delete(delete_session),
        )
        .route("/sessions/:name/start", post(start_session))
        .route("/sessions/:name/stop", post(stop_session))
        .route("/sessions/:name/jump", post(jump_session))
        .route("/editor/state", get(get_editor_state))
        .route("/editor/armadas", post(create_armada))
        .route("/editor/armadas/:id", axum::routing::patch(patch_armada))
        .route("/editor/fleets", post(create_fleet))
        .route("/editor/fleets/:id", axum::routing::patch(patch_fleet))
        .route("/editor/flotillas", post(create_flotilla))
        .route(
            "/editor/flotillas/:id",
            axum::routing::patch(patch_flotilla),
        )
        .route("/editor/crews", post(create_crew_bundle))
        .route("/editor/crews/:id", axum::routing::patch(patch_crew_bundle))
        .route("/editor/crews/:id/clone", post(clone_crew_bundle))
        .route("/editor/crew-refs/:id/move", post(move_crew_ref))
        .route("/editor/crew-refs/:id", delete(unlink_crew_ref))
        .route("/editor/import-discovery", post(import_discovery_session))
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

#[derive(Debug, Deserialize)]
struct CreateArmadaRequest {
    name: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PatchArmadaRequest {
    name: Option<String>,
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateFleetRequest {
    armada_id: i64,
    name: String,
    color: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PatchFleetRequest {
    armada_id: Option<i64>,
    name: Option<String>,
    color: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateFlotillaRequest {
    fleet_id: i64,
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct PatchFlotillaRequest {
    fleet_id: Option<i64>,
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CrewMemberPayload {
    member_id: String,
    role: String,
    ai_provider: String,
    model: String,
    startup_prompts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CrewVariantPayload {
    host_id: i64,
    repo_url: Option<String>,
    branch_ref: Option<String>,
    root_path: String,
    config_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CrewPlacementPayload {
    armada_id: i64,
    fleet_id: i64,
    flotilla_id: Option<i64>,
    alias_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateCrewBundleRequest {
    crew_name: String,
    crew_ulid: String,
    members: Vec<CrewMemberPayload>,
    variants: Vec<CrewVariantPayload>,
    placement: CrewPlacementPayload,
}

#[derive(Debug, Deserialize, Default)]
struct PatchCrewBundleRequest {
    crew_ulid: Option<String>,
    members: Option<Vec<CrewMemberPayload>>,
    variants: Option<Vec<CrewVariantPayload>>,
}

#[derive(Debug, Deserialize)]
struct CloneCrewBundleRequest {
    crew_name: String,
    crew_ulid: String,
    placement: CrewPlacementPayload,
}

#[derive(Debug, Deserialize)]
struct MoveCrewRefRequest {
    armada_id: i64,
    fleet_id: i64,
    flotilla_id: Option<i64>,
    alias_name: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct ImportDiscoveryRequest {
    session_name: String,
    crew_name: Option<String>,
    crew_ulid: Option<String>,
    armada_id: i64,
    fleet_id: i64,
    flotilla_id: Option<i64>,
    alias_name: Option<String>,
    host_id: Option<i64>,
    repo_url: Option<String>,
    branch_ref: Option<String>,
    root_path: String,
}

#[derive(Debug, Serialize)]
struct RuntimeCrewSummary {
    crew_id: i64,
    crew_name: String,
    crew_ulid: String,
    host_id: i64,
    root_path: String,
    repo_url: Option<String>,
    branch_ref: Option<String>,
    status: String,
    discovered: bool,
    pane_count: usize,
    binding_valid: bool,
    binding_error: Option<String>,
}

#[derive(Debug, Clone)]
struct CrewVariantBinding {
    host_id: i64,
    root_path: String,
}

#[derive(Debug, Serialize)]
struct EditorStateResponse {
    armadas: Vec<EditorArmada>,
    fleets: Vec<EditorFleet>,
    flotillas: Vec<EditorFlotilla>,
    crews: Vec<EditorCrew>,
    crew_refs: Vec<EditorCrewRef>,
}

#[derive(Debug, Serialize)]
struct EditorArmada {
    id: i64,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct EditorFleet {
    id: i64,
    armada_id: i64,
    name: String,
    color: Option<String>,
}

#[derive(Debug, Serialize)]
struct EditorFlotilla {
    id: i64,
    fleet_id: i64,
    name: String,
}

#[derive(Debug, Serialize)]
struct EditorCrew {
    id: i64,
    crew_name: String,
    crew_ulid: String,
    member_count: i64,
    variant_count: i64,
}

#[derive(Debug, Serialize)]
struct EditorCrewRef {
    id: i64,
    crew_id: i64,
    armada_id: i64,
    fleet_id: i64,
    flotilla_id: Option<i64>,
    alias_name: Option<String>,
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

async fn health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<ErrorResponse>)> {
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
            return Err(internal_error("failed to query session count".to_string()));
        }
        Err(err) => {
            tracing::warn!("health join error: {err}");
            return Err(internal_error(
                "failed to join health query task".to_string(),
            ));
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
) -> Result<Json<Vec<SessionSummary>>, (StatusCode, Json<ErrorResponse>)> {
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
                return Err(internal_error(
                    "failed to read session definitions".to_string(),
                ));
            }
            Err(err) => {
                tracing::warn!("list_sessions join error: {err}");
                return Err(internal_error(
                    "failed to join list_sessions task".to_string(),
                ));
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
) -> Result<Json<SessionDetail>, (StatusCode, Json<ErrorResponse>)> {
    let row = {
        let state = Arc::clone(&state);
        let name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let db = state.db.lock().expect("db lock");
            db::get_session_for_host(&db, state.host_id, &name)
        })
        .await
        .map_err(|_| internal_error("failed to join get_session read task".to_string()))?;
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
    if let Some(binding) = fetch_preferred_crew_variant_binding(Arc::clone(&state), &name).await? {
        if let Err(message) = validate_crew_variant_binding(&binding, state.host_id) {
            return Err(bad_request("invalid_crew_variant_binding", message));
        }
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
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let definition = {
        let state = Arc::clone(&state);
        let name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let db_conn = state.db.lock().expect("db lock");
            db::get_session_for_host(&db_conn, state.host_id, &name)
        })
        .await
        .map_err(|_| internal_error("failed to join stop_session read task".to_string()))?;
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
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
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
    .map_err(|_| internal_error("failed to join jump_session read task".to_string()))?
    .map_err(|_| internal_error("failed to read jump target definition".to_string()))?;

    let (session, host) = session_data;
    if session.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("session '{name}' not found"),
            }),
        ));
    }
    let host = host.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("host '{host_id}' not found"),
            }),
        )
    })?;

    let terminal = req
        .terminal
        .or_else(|| state.config.daemon.default_terminal.clone())
        .unwrap_or_else(|| "iterm2".to_string());

    let target = if host.is_local {
        HostTarget::Local
    } else {
        HostTarget::Remote {
            user: host.ssh_user.ok_or_else(|| {
                bad_request(
                    "invalid_host",
                    format!("host '{}' is missing ssh_user", host.name),
                )
            })?,
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

async fn create_armada(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateArmadaRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let armada = definition_writer::NewArmada {
        name: req.name.clone(),
        description: req.description,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_armada(&db_conn, &armada)
    })
    .await
    .map_err(|_| internal_error("failed to join create_armada task".to_string()))?;

    match result {
        Ok(id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("armada '{id}' created"),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_armada(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<PatchArmadaRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let patch = definition_writer::ArmadaPatch {
        name: req.name,
        description: req.description,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_armada(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_armada task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("armada '{id}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("armada '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn create_fleet(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateFleetRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let fleet = definition_writer::NewFleet {
        armada_id: req.armada_id,
        name: req.name.clone(),
        color: req.color,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_fleet(&db_conn, &fleet)
    })
    .await
    .map_err(|_| internal_error("failed to join create_fleet task".to_string()))?;

    match result {
        Ok(id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("fleet '{id}' created"),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_fleet(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<PatchFleetRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let patch = definition_writer::FleetPatch {
        armada_id: req.armada_id,
        name: req.name,
        color: req.color,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_fleet(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_fleet task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("fleet '{id}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("fleet '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn create_flotilla(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateFlotillaRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let flotilla = definition_writer::NewFlotilla {
        fleet_id: req.fleet_id,
        name: req.name.clone(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_flotilla(&db_conn, &flotilla)
    })
    .await
    .map_err(|_| internal_error("failed to join create_flotilla task".to_string()))?;

    match result {
        Ok(id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("flotilla '{id}' created"),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_flotilla(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<PatchFlotillaRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let patch = definition_writer::FlotillaPatch {
        fleet_id: req.fleet_id,
        name: req.name,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_flotilla(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_flotilla task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("flotilla '{id}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("flotilla '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn create_crew_bundle(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateCrewBundleRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let members = req
        .members
        .iter()
        .map(map_member_payload)
        .collect::<Result<Vec<_>, _>>()?;
    let variants = req
        .variants
        .iter()
        .map(map_variant_payload)
        .collect::<Result<Vec<_>, _>>()?;
    let placement = map_placement_payload(&req.placement);

    let bundle = definition_writer::NewCrewBundle {
        crew_name: req.crew_name,
        crew_ulid: req.crew_ulid,
        members,
        variants,
        placement,
    };

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::create_crew_bundle(&db_conn, &bundle)
    })
    .await
    .map_err(|_| internal_error("failed to join create_crew_bundle task".to_string()))?;

    match result {
        Ok(crew_id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("crew '{crew_id}' created"),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn patch_crew_bundle(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<PatchCrewBundleRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let editing_roster = req.members.is_some() || req.variants.is_some();
    if editing_roster {
        if let Some(crew_name) = fetch_crew_name(Arc::clone(&state), id).await? {
            let live = tmux::live_sessions().await.unwrap_or_default();
            if live.contains_key(&crew_name) {
                return Err((
                    StatusCode::CONFLICT,
                    Json(ErrorResponse {
                        ok: false,
                        code: "running_edit_forbidden".to_string(),
                        message: "running crew roster/prompt edits are not allowed".to_string(),
                    }),
                ));
            }
        }
    }

    let members = req
        .members
        .as_ref()
        .map(|items| {
            items
                .iter()
                .map(map_member_payload)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let variants = req
        .variants
        .as_ref()
        .map(|items| {
            items
                .iter()
                .map(map_variant_payload)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let patch = definition_writer::CrewBundlePatch {
        crew_ulid: req.crew_ulid,
        members,
        variants,
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::patch_crew_bundle(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join patch_crew_bundle task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("crew '{id}' updated"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("crew '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn clone_crew_bundle(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<CloneCrewBundleRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let request = definition_writer::CloneCrewRequest {
        crew_name: req.crew_name,
        crew_ulid: req.crew_ulid,
        placement: map_placement_payload(&req.placement),
    };
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::clone_crew(&db_conn, id, &request)
    })
    .await
    .map_err(|_| internal_error("failed to join clone_crew_bundle task".to_string()))?;

    match result {
        Ok(new_id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("crew cloned to '{new_id}'"),
        })),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn move_crew_ref(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<MoveCrewRefRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let patch = definition_writer::MoveCrewRefPatch {
        armada_id: req.armada_id,
        fleet_id: req.fleet_id,
        flotilla_id: req.flotilla_id,
        alias_name: req.alias_name,
    };

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::move_crew_ref(&db_conn, id, &patch)
    })
    .await
    .map_err(|_| internal_error("failed to join move_crew_ref task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("crew ref '{id}' moved"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("crew ref '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn unlink_crew_ref(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        definition_writer::unlink_crew_ref(&db_conn, id)
    })
    .await
    .map_err(|_| internal_error("failed to join unlink_crew_ref task".to_string()))?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("crew ref '{id}' unlinked"),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                ok: false,
                code: "not_found".to_string(),
                message: format!("crew ref '{id}' not found"),
            }),
        )),
        Err(err) => Err(map_write_error(err)),
    }
}

async fn get_editor_state(
    State(state): State<Arc<AppState>>,
) -> Result<Json<EditorStateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<EditorStateResponse> {
        let db_conn = state.db.lock().expect("db lock");

        let armadas = {
            let mut stmt =
                db_conn.prepare("SELECT id, name, description FROM armadas ORDER BY name")?;
            let mapped = stmt.query_map([], |r| {
                Ok(EditorArmada {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let fleets = {
            let mut stmt =
                db_conn.prepare("SELECT id, armada_id, name, color FROM fleets ORDER BY name")?;
            let mapped = stmt.query_map([], |r| {
                Ok(EditorFleet {
                    id: r.get(0)?,
                    armada_id: r.get(1)?,
                    name: r.get(2)?,
                    color: r.get(3)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let flotillas = {
            let mut stmt = db_conn.prepare("SELECT id, fleet_id, name FROM flotillas ORDER BY name")?;
            let mapped = stmt.query_map([], |r| {
                Ok(EditorFlotilla {
                    id: r.get(0)?,
                    fleet_id: r.get(1)?,
                    name: r.get(2)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let crews = {
            let mut stmt = db_conn.prepare(
                "SELECT c.id, c.crew_name, c.crew_ulid,
                        (SELECT COUNT(*) FROM crew_members cm WHERE cm.crew_id = c.id) AS member_count,
                        (SELECT COUNT(*) FROM crew_variants cv WHERE cv.crew_id = c.id) AS variant_count
                 FROM crews c
                 ORDER BY c.crew_name",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok(EditorCrew {
                    id: r.get(0)?,
                    crew_name: r.get(1)?,
                    crew_ulid: r.get(2)?,
                    member_count: r.get(3)?,
                    variant_count: r.get(4)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let crew_refs = {
            let mut stmt = db_conn.prepare(
                "SELECT id, crew_id, armada_id, fleet_id, flotilla_id, alias_name
                 FROM crew_refs
                 ORDER BY armada_id, fleet_id, id",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok(EditorCrewRef {
                    id: r.get(0)?,
                    crew_id: r.get(1)?,
                    armada_id: r.get(2)?,
                    fleet_id: r.get(3)?,
                    flotilla_id: r.get(4)?,
                    alias_name: r.get(5)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(EditorStateResponse {
            armadas,
            fleets,
            flotillas,
            crews,
            crew_refs,
        })
    })
    .await
    .map_err(|_| internal_error("failed to join get_editor_state task".to_string()))?;

    match result {
        Ok(body) => Ok(Json(body)),
        Err(err) => Err(internal_error(format!(
            "failed to load editor state: {err}"
        ))),
    }
}

async fn get_dashboard_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DashboardConfigResponse>, (StatusCode, Json<ErrorResponse>)> {
    let hosts = fetch_hosts(Arc::clone(&state))
        .await
        .map_err(|_| internal_error("failed to read hosts for dashboard config".to_string()))?;
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

async fn get_unregistered_discovery(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::runtime::DiscoverySession>>, (StatusCode, Json<ErrorResponse>)> {
    let discovery_rows = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.discovery_rows()
    };
    let registered_crew_names = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || -> anyhow::Result<HashSet<String>> {
            let db_conn = state.db.lock().expect("db lock");
            let mut stmt = db_conn.prepare("SELECT crew_name FROM crews")?;
            let mapped = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let values = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(values.into_iter().collect::<HashSet<_>>())
        }
    })
    .await
    .map_err(|_| internal_error("failed to join get_unregistered_discovery task".to_string()))?
    .map_err(|err| internal_error(format!("failed to list registered crews: {err}")))?;

    let rows = discovery_rows
        .into_iter()
        .filter(|row| !registered_crew_names.contains(&row.name))
        .collect::<Vec<_>>();
    Ok(Json(rows))
}

async fn list_runtime_crews(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RuntimeCrewSummary>>, (StatusCode, Json<ErrorResponse>)> {
    #[derive(Debug)]
    struct CrewVariantRow {
        crew_id: i64,
        crew_name: String,
        crew_ulid: String,
        host_id: i64,
        repo_url: Option<String>,
        branch_ref: Option<String>,
        root_path: String,
    }

    let variant_rows = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || -> anyhow::Result<Vec<CrewVariantRow>> {
            let db_conn = state.db.lock().expect("db lock");
            let mut stmt = db_conn.prepare(
                "SELECT c.id, c.crew_name, c.crew_ulid, cv.host_id, cv.repo_url, cv.branch_ref, cv.root_path
                 FROM crews c
                 JOIN crew_variants cv ON cv.crew_id = c.id
                 ORDER BY c.crew_name, cv.id",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok(CrewVariantRow {
                    crew_id: r.get(0)?,
                    crew_name: r.get(1)?,
                    crew_ulid: r.get(2)?,
                    host_id: r.get(3)?,
                    repo_url: r.get(4)?,
                    branch_ref: r.get(5)?,
                    root_path: r.get(6)?,
                })
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>().map_err(anyhow::Error::from)
        }
    })
    .await
    .map_err(|_| internal_error("failed to join list_runtime_crews task".to_string()))?
    .map_err(|err| internal_error(format!("failed to list crew variants: {err}")))?;

    let rows = {
        let runtime = state.runtime.lock().expect("runtime lock");
        let discovery_rows = runtime.discovery_rows();
        variant_rows
            .into_iter()
            .map(|row| {
                let discovery = discovery_rows
                    .iter()
                    .find(|item| item.name == row.crew_name);
                let status = runtime
                    .session(&row.crew_name)
                    .map(|entry| entry.status.clone())
                    .or_else(|| discovery.map(|item| derive_discovery_status(&item.panes)))
                    .unwrap_or_else(|| "stopped".to_string());
                let binding = CrewVariantBinding {
                    host_id: row.host_id,
                    root_path: row.root_path.clone(),
                };
                let binding_error = validate_crew_variant_binding(&binding, state.host_id).err();
                RuntimeCrewSummary {
                    crew_id: row.crew_id,
                    crew_name: row.crew_name,
                    crew_ulid: row.crew_ulid,
                    host_id: row.host_id,
                    root_path: row.root_path,
                    repo_url: row.repo_url,
                    branch_ref: row.branch_ref,
                    status,
                    discovered: discovery.is_some(),
                    pane_count: discovery.map(|item| item.panes.len()).unwrap_or(0),
                    binding_valid: binding_error.is_none(),
                    binding_error,
                }
            })
            .collect::<Vec<_>>()
    };

    Ok(Json(rows))
}

async fn import_discovery_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImportDiscoveryRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session_name = req.session_name.trim().to_string();
    if session_name.is_empty() {
        return Err(bad_request(
            "invalid_session_name",
            "session_name is required".to_string(),
        ));
    }
    let root_path = req.root_path.trim().to_string();
    if root_path.is_empty() {
        return Err(bad_request(
            "invalid_root_path",
            "root_path is required".to_string(),
        ));
    }

    let discovery = {
        let runtime = state.runtime.lock().expect("runtime lock");
        runtime.discovery_rows()
    };
    let row = discovery
        .into_iter()
        .find(|item| item.name == session_name)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    ok: false,
                    code: "discovery_not_found".to_string(),
                    message: format!("discovered session '{session_name}' not found"),
                }),
            )
        })?;
    if row.panes.is_empty() {
        return Err(bad_request(
            "empty_discovery_session",
            "cannot import a discovered session with zero panes".to_string(),
        ));
    }

    let host_id = req.host_id.unwrap_or(state.host_id);
    let crew_name = req
        .crew_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&session_name)
        .to_string();
    let crew_ulid = req
        .crew_ulid
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| generate_import_crew_ulid(&crew_name));

    let members = map_discovery_members(&row.panes);
    let bundle = definition_writer::NewCrewBundle {
        crew_name: crew_name.clone(),
        crew_ulid,
        members,
        variants: vec![definition_writer::CrewVariantInput {
            host_id,
            repo_url: req.repo_url,
            branch_ref: req.branch_ref,
            root_path: root_path.clone(),
            config_json: None,
        }],
        placement: definition_writer::CrewPlacementInput {
            armada_id: req.armada_id,
            fleet_id: req.fleet_id,
            flotilla_id: req.flotilla_id,
            alias_name: req.alias_name,
        },
    };

    let created = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || {
            let db_conn = state.db.lock().expect("db lock");
            definition_writer::create_crew_bundle(&db_conn, &bundle)
        }
    })
    .await
    .map_err(|_| internal_error("failed to join import_discovery_session task".to_string()))?;

    match created {
        Ok(crew_id) => Ok(Json(ActionResponse {
            ok: true,
            message: format!(
                "imported discovered session '{}' into crew '{}' (id={})",
                session_name, crew_name, crew_id
            ),
        })),
        Err(err) => Err(map_write_error(err)),
    }
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

async fn fetch_preferred_crew_variant_binding(
    state: Arc<AppState>,
    crew_name: &str,
) -> Result<Option<CrewVariantBinding>, (StatusCode, Json<ErrorResponse>)> {
    let crew_name = crew_name.to_string();
    let host_id = state.host_id;
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT cv.host_id, cv.root_path
                 FROM crews c
                 JOIN crew_variants cv ON cv.crew_id = c.id
                 WHERE c.crew_name = ?1
                 ORDER BY CASE WHEN cv.host_id = ?2 THEN 0 ELSE 1 END, cv.id
                 LIMIT 1",
                rusqlite::params![crew_name, host_id],
                |r| {
                    Ok(CrewVariantBinding {
                        host_id: r.get(0)?,
                        root_path: r.get(1)?,
                    })
                },
            )
            .optional()
    })
    .await
    .map_err(|_| {
        internal_error("failed to join fetch_preferred_crew_variant_binding task".to_string())
    })?;

    result.map_err(|_| internal_error("failed to read crew variant binding".to_string()))
}

fn validate_crew_variant_binding(
    binding: &CrewVariantBinding,
    daemon_host_id: i64,
) -> Result<(), String> {
    if binding.host_id != daemon_host_id {
        return Err(format!(
            "crew variant host_id {} does not match local daemon host_id {}",
            binding.host_id, daemon_host_id
        ));
    }
    let root_path = binding.root_path.trim();
    if root_path.is_empty() {
        return Err("crew variant root_path is required".to_string());
    }
    let path = FsPath::new(root_path);
    if !path.exists() {
        return Err(format!(
            "crew variant root_path does not exist: {root_path}"
        ));
    }
    if !path.is_dir() {
        return Err(format!(
            "crew variant root_path is not a directory: {root_path}"
        ));
    }
    Ok(())
}

fn derive_discovery_status(panes: &[crate::tmux::PaneInfo]) -> String {
    if panes.is_empty() {
        return "stopped".to_string();
    }
    let any_active = panes.iter().any(|pane| {
        matches!(
            pane.status.to_ascii_lowercase().as_str(),
            "active" | "running" | "stuck"
        )
    });
    if any_active {
        "running".to_string()
    } else {
        "idle".to_string()
    }
}

fn generate_import_crew_ulid(crew_name: &str) -> String {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let label = sanitize_identifier(crew_name);
    format!(
        "import-{}-{}",
        if label.is_empty() { "crew" } else { &label },
        suffix
    )
}

fn sanitize_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn map_discovery_members(
    panes: &[crate::tmux::PaneInfo],
) -> Vec<definition_writer::CrewMemberInput> {
    let mut used = HashSet::new();
    panes
        .iter()
        .enumerate()
        .map(|(idx, pane)| {
            let base = sanitize_identifier(&pane.name);
            let mut candidate = if base.is_empty() {
                format!("member-{}", idx + 1)
            } else {
                base.clone()
            };
            let mut serial = 2u32;
            while !used.insert(candidate.clone()) {
                let root = if base.is_empty() {
                    format!("member-{}", idx + 1)
                } else {
                    base.clone()
                };
                candidate = format!("{root}-{serial}");
                serial += 1;
            }

            let (role, ai_provider, model) = if idx == 0 {
                ("captain", "claude", "claude-opus")
            } else {
                ("mate", "codex", "codex-high")
            };

            definition_writer::CrewMemberInput {
                member_id: candidate,
                role: role.to_string(),
                ai_provider: ai_provider.to_string(),
                model: model.to_string(),
                startup_prompts_json: "[]".to_string(),
            }
        })
        .collect::<Vec<_>>()
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

fn map_member_payload(
    payload: &CrewMemberPayload,
) -> Result<definition_writer::CrewMemberInput, (StatusCode, Json<ErrorResponse>)> {
    let startup_prompts_json = serde_json::to_string(&payload.startup_prompts).map_err(|_| {
        bad_request(
            "invalid_startup_prompts",
            "startup_prompts could not be serialized".to_string(),
        )
    })?;

    Ok(definition_writer::CrewMemberInput {
        member_id: payload.member_id.clone(),
        role: payload.role.clone(),
        ai_provider: payload.ai_provider.clone(),
        model: payload.model.clone(),
        startup_prompts_json,
    })
}

fn map_variant_payload(
    payload: &CrewVariantPayload,
) -> Result<definition_writer::CrewVariantInput, (StatusCode, Json<ErrorResponse>)> {
    if payload.root_path.trim().is_empty() {
        return Err(bad_request(
            "invalid_root_path",
            "root_path is required".to_string(),
        ));
    }

    let config_json = payload
        .config_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| {
            bad_request(
                "invalid_variant_config",
                "invalid variant config_json".to_string(),
            )
        })?;

    Ok(definition_writer::CrewVariantInput {
        host_id: payload.host_id,
        repo_url: payload.repo_url.clone(),
        branch_ref: payload.branch_ref.clone(),
        root_path: payload.root_path.clone(),
        config_json,
    })
}

fn map_placement_payload(payload: &CrewPlacementPayload) -> definition_writer::CrewPlacementInput {
    definition_writer::CrewPlacementInput {
        armada_id: payload.armada_id,
        fleet_id: payload.fleet_id,
        flotilla_id: payload.flotilla_id,
        alias_name: payload.alias_name.clone(),
    }
}

async fn fetch_crew_name(
    state: Arc<AppState>,
    crew_id: i64,
) -> Result<Option<String>, (StatusCode, Json<ErrorResponse>)> {
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().expect("db lock");
        db_conn
            .query_row(
                "SELECT crew_name FROM crews WHERE id = ?1",
                rusqlite::params![crew_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
    })
    .await
    .map_err(|_| internal_error("failed to join fetch_crew_name task".to_string()))?;
    result.map_err(|_| internal_error("failed to read crew name".to_string()))
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
