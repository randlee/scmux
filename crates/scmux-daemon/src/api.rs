use axum::{
    extract::{Path, Request, State},
    http::{header, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::db;
use crate::tmux::{self, HostTarget};
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    let middleware_state = Arc::clone(&state);
    Router::new()
        .route("/", get(dashboard_index))
        .route("/dashboard.js", get(dashboard_js))
        .route("/react.min.js", get(react_js))
        .route("/react-dom.min.js", get(react_dom_js))
        .route("/health", get(health))
        .route("/hosts", get(list_hosts))
        .route("/dashboard-config.json", get(get_dashboard_config))
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

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    uptime_secs: u64,
    session_count: i64,
    db_path: String,
    version: &'static str,
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
    recent_events: Vec<EventRow>,
}

#[derive(Serialize)]
struct EventRow {
    event: String,
    trigger: String,
    note: Option<String>,
    occurred_at: String,
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
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

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    let db_path = state.db_path.clone();
    let session_count: i64 = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        db.query_row("SELECT COUNT(*) FROM sessions WHERE enabled = 1", [], |r| {
            r.get(0)
        })
        .unwrap_or(0)
    })
    .await
    .unwrap_or(0);

    Json(HealthResponse {
        status: "ok",
        uptime_secs,
        session_count,
        db_path,
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Vec<SessionSummary>> {
    let sessions = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<SessionSummary>> {
        let db = state.db.lock().unwrap();
        let ci_by_session = load_ci_by_session(&db)?;
        let atm_by_session = if state.atm_available.load(Ordering::Relaxed) {
            load_atm_by_session_name(&db)?
        } else {
            HashMap::new()
        };
        let mut stmt = db.prepare(
            "SELECT s.id, s.name, s.project, s.host_id, s.cron_schedule, s.auto_start,
                    COALESCE(ss.status, 'stopped') as status,
                    COALESCE(ss.panes_json, '[]') as panes_json,
                    ss.polled_at
                 FROM sessions s
                 LEFT JOIN session_status ss ON ss.session_id = s.id
                 WHERE s.enabled = 1
                 ORDER BY s.host_id, s.project, s.name",
        )?;

        let rows = stmt.query_map([], |r| {
            let panes_str: String = r.get(7)?;
            let id: i64 = r.get(0)?;
            let name: String = r.get(1)?;
            Ok(SessionSummary {
                id,
                name: name.clone(),
                project: r.get(2)?,
                host_id: r.get(3)?,
                cron_schedule: r.get(4)?,
                auto_start: r.get(5)?,
                status: r.get(6)?,
                panes: serde_json::from_str(&panes_str).unwrap_or(serde_json::json!([])),
                polled_at: r.get(8)?,
                session_ci: ci_by_session.get(&id).cloned().unwrap_or_default(),
                atm: atm_by_session.get(&name).cloned(),
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default();

    Json(sessions)
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SessionDetail>, StatusCode> {
    let result = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();

        let row = db
            .query_row(
                "SELECT s.id, s.host_id, s.name, s.project, s.cron_schedule, s.auto_start,
                    s.config_json,
                    COALESCE(ss.status, 'stopped'),
                    COALESCE(ss.panes_json, '[]'),
                    ss.polled_at
             FROM sessions s
             LEFT JOIN session_status ss ON ss.session_id = s.id
             WHERE s.name = ?1 AND s.host_id = ?2 AND s.enabled = 1",
                params![name, state.host_id],
                |r| {
                    let panes_str: String = r.get(8)?;
                    let config_str: String = r.get(6)?;
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, bool>(5)?,
                        config_str,
                        r.get::<_, String>(7)?,
                        panes_str,
                        r.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;

        let (
            id,
            host_id,
            name,
            project,
            cron_schedule,
            auto_start,
            config_str,
            status,
            panes_str,
            polled_at,
        ) = row;
        let session_ci =
            load_ci_for_session(&db, id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let atm = if state.atm_available.load(Ordering::Relaxed) {
            load_atm_for_session(&db, &name).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            None
        };

        let mut estmt = db
            .prepare(
                "SELECT event, trigger, note, occurred_at
             FROM session_events
             WHERE session_id = ?1
             ORDER BY occurred_at DESC LIMIT 20",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let events: Vec<EventRow> = estmt
            .query_map(params![id], |r| {
                Ok(EventRow {
                    event: r.get(0)?,
                    trigger: r.get(1)?,
                    note: r.get(2)?,
                    occurred_at: r.get(3)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        Ok::<_, StatusCode>(Json(SessionDetail {
            summary: SessionSummary {
                id,
                name,
                project,
                host_id,
                cron_schedule,
                auto_start,
                status,
                panes: serde_json::from_str(&panes_str).unwrap_or(serde_json::json!([])),
                polled_at,
                session_ci,
                atm,
            },
            config_json: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
            recent_events: events,
        }))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    result
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let host_id = req.host_id.unwrap_or(state.host_id);
    let config_json =
        serde_json::to_string(&req.config_json).map_err(|_| StatusCode::BAD_REQUEST)?;

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
        let db_conn = state.db.lock().unwrap();
        db::create_session(&db_conn, &new_session)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(_) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{}' created", req.name),
        })),
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("UNIQUE constraint failed") {
                return Err(StatusCode::CONFLICT);
            }
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

async fn patch_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<PatchSessionRequest>,
) -> Result<Json<ActionResponse>, StatusCode> {
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

    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().unwrap();
        db::update_session(&db_conn, state.host_id, &name, &patch)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{response_name}' updated"),
        })),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let response_name = name.clone();
    let result = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().unwrap();
        db::soft_delete_session(&db_conn, state.host_id, &name)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(true) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{response_name}' disabled"),
        })),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn start_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let state2 = Arc::clone(&state);
    let name2 = name.clone();
    let config_json = tokio::task::spawn_blocking(move || {
        let db_conn = state2.db.lock().unwrap();
        db_conn
            .query_row(
                "SELECT config_json FROM sessions WHERE name = ?1 AND host_id = ?2 AND enabled = 1",
                params![name2, state2.host_id],
                |r| r.get::<_, String>(0),
            )
            .map_err(|_| StatusCode::NOT_FOUND)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    let action = tmux::start_session(&name, &config_json).await;
    log_action_result(&state, &name, "started", "manual", action.as_ref().err())
        .await
        .ok();

    match action {
        Ok(()) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{name}' started"),
        })),
        Err(e) => Ok(Json(ActionResponse {
            ok: false,
            message: e.to_string(),
        })),
    }
}

async fn stop_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let state2 = Arc::clone(&state);
    let name2 = name.clone();
    let exists = tokio::task::spawn_blocking(move || {
        let db_conn = state2.db.lock().unwrap();
        db_conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sessions WHERE name = ?1 AND host_id = ?2 AND enabled = 1",
                params![name2, state2.host_id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(anyhow::Error::from)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    ;
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let action = tmux::stop_session(&name).await;
    log_action_result(&state, &name, "stopped", "manual", action.as_ref().err())
        .await
        .ok();

    match action {
        Ok(()) => Ok(Json(ActionResponse {
            ok: true,
            message: format!("session '{name}' stopped"),
        })),
        Err(e) => Ok(Json(ActionResponse {
            ok: false,
            message: e.to_string(),
        })),
    }
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
        let db_conn = state2.db.lock().unwrap();
        db_conn
            .query_row(
                "SELECT s.id, s.name, h.address, h.is_local, h.ssh_user, h.api_port
                 FROM sessions s
                 INNER JOIN hosts h ON h.id = s.host_id
                 WHERE s.name = ?1 AND s.host_id = ?2 AND s.enabled = 1
                 LIMIT 1",
                params![name2, host_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, u16>(5)?,
                    ))
                },
            )
            .map_err(|_| StatusCode::NOT_FOUND)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    let (session_id, _session_name, host, is_local, ssh_user, _api_port) = session_data;
    let terminal = req
        .terminal
        .or_else(|| state.config.daemon.default_terminal.clone())
        .unwrap_or_else(|| "iterm2".to_string());

    let target = if is_local {
        HostTarget::Local
    } else {
        HostTarget::Remote {
            user: ssh_user.ok_or(StatusCode::BAD_REQUEST)?,
            host,
        }
    };

    let result = tmux::jump_session(target, &name, &terminal).await;
    let note = result.as_ref().err().map(|err| err.to_string());
    let event = if result.is_ok() { "jumped" } else { "failed" };
    log_session_event(&state, session_id, event, "manual", note.as_deref())
        .await
        .ok();

    match result {
        Ok(message) => Ok(Json(ActionResponse { ok: true, message })),
        Err(e) => Ok(Json(ActionResponse {
            ok: false,
            message: e.to_string(),
        })),
    }
}

async fn list_hosts(State(state): State<Arc<AppState>>) -> Json<Vec<HostSummary>> {
    let hosts = fetch_hosts(state).await.unwrap_or_default();
    Json(hosts)
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

async fn fetch_hosts(state: Arc<AppState>) -> anyhow::Result<Vec<HostSummary>> {
    let reachability = {
        let map = state.reachability.lock().expect("reachability lock");
        map.clone()
    };
    let hosts = tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().unwrap();
        let mut stmt = db_conn.prepare(
            "SELECT id, name, address, ssh_user, api_port, is_local, last_seen
             FROM hosts ORDER BY is_local DESC, name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let is_local: bool = r.get(5)?;
                let address: String = r.get(2)?;
                let api_port: u16 = r.get(4)?;
                let db_last_seen: Option<String> = r.get(6)?;
                let reach = reachability.get(&id);
                let reachable = if is_local {
                    true
                } else {
                    reach.map(|entry| entry.reachable).unwrap_or(false)
                };
                let last_seen = reach
                    .and_then(|entry| entry.last_seen.clone())
                    .or(db_last_seen);
                Ok(HostSummary {
                    id,
                    name: r.get(1)?,
                    address: address.clone(),
                    ssh_user: r.get(3)?,
                    api_port,
                    is_local,
                    last_seen,
                    reachable,
                    url: format!("http://{address}:{api_port}"),
                })
            })?
            .filter_map(|row| row.ok())
            .collect::<Vec<_>>();
        Ok::<_, anyhow::Error>(rows)
    })
    .await??;
    Ok(hosts)
}

fn load_ci_by_session(
    db: &rusqlite::Connection,
) -> anyhow::Result<HashMap<i64, Vec<SessionCiSummary>>> {
    let mut stmt = db.prepare(
        "SELECT session_id, provider, status, data_json, tool_message, polled_at, next_poll_at
         FROM session_ci
         ORDER BY session_id, provider",
    )?;
    let mut map: HashMap<i64, Vec<SessionCiSummary>> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        let data_json: Option<String> = r.get(3)?;
        Ok((
            r.get::<_, i64>(0)?,
            SessionCiSummary {
                provider: r.get(1)?,
                status: r.get(2)?,
                data_json: data_json.and_then(|raw| serde_json::from_str(&raw).ok()),
                tool_message: r.get(4)?,
                polled_at: r.get(5)?,
                next_poll_at: r.get(6)?,
            },
        ))
    })?;

    for row in rows {
        let (session_id, entry) = row?;
        map.entry(session_id).or_default().push(entry);
    }
    Ok(map)
}

fn load_ci_for_session(
    db: &rusqlite::Connection,
    session_id: i64,
) -> anyhow::Result<Vec<SessionCiSummary>> {
    let mut stmt = db.prepare(
        "SELECT provider, status, data_json, tool_message, polled_at, next_poll_at
         FROM session_ci
         WHERE session_id = ?1
         ORDER BY provider",
    )?;
    let rows = stmt
        .query_map(params![session_id], |r| {
            let data_json: Option<String> = r.get(2)?;
            Ok(SessionCiSummary {
                provider: r.get(0)?,
                status: r.get(1)?,
                data_json: data_json.and_then(|raw| serde_json::from_str(&raw).ok()),
                tool_message: r.get(3)?,
                polled_at: r.get(4)?,
                next_poll_at: r.get(5)?,
            })
        })?
        .filter_map(|row| row.ok())
        .collect::<Vec<_>>();
    Ok(rows)
}

fn load_atm_by_session_name(
    db: &rusqlite::Connection,
) -> anyhow::Result<HashMap<String, SessionAtmSummary>> {
    let rows = db::list_session_atm(db)?;
    let mut map = HashMap::new();
    for row in rows {
        map.insert(
            row.session_name,
            SessionAtmSummary {
                state: row.state,
                last_transition: row.last_transition,
            },
        );
    }
    Ok(map)
}

fn load_atm_for_session(
    db: &rusqlite::Connection,
    session_name: &str,
) -> anyhow::Result<Option<SessionAtmSummary>> {
    let map = load_atm_by_session_name(db)?;
    Ok(map.get(session_name).cloned())
}

async fn log_action_result(
    state: &Arc<AppState>,
    name: &str,
    success_event: &str,
    trigger: &str,
    err: Option<&anyhow::Error>,
) -> anyhow::Result<()> {
    let note = err.map(|e| e.to_string());
    let event = if err.is_some() {
        "failed".to_string()
    } else {
        success_event.to_string()
    };
    let trigger = trigger.to_string();
    let name = name.to_string();
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let db_conn = state.db.lock().unwrap();
        let Some(session_id) = db::session_id(&db_conn, state.host_id, &name)? else {
            return Ok(());
        };
        db::log_session_event(&db_conn, session_id, &event, &trigger, note.as_deref())?;
        Ok(())
    })
    .await??;
    Ok(())
}

async fn log_session_event(
    state: &Arc<AppState>,
    session_id: i64,
    event: &str,
    trigger: &str,
    note: Option<&str>,
) -> anyhow::Result<()> {
    let event = event.to_string();
    let trigger = trigger.to_string();
    let note = note.map(ToOwned::to_owned);
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let db_conn = state.db.lock().unwrap();
        db::log_session_event(&db_conn, session_id, &event, &trigger, note.as_deref())
    })
    .await??;
    Ok(())
}
