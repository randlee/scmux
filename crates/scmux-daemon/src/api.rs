use std::sync::Arc;
use axum::{
    Router,
    routing::{get, post},
    extract::{State, Path},
    response::Json,
    http::StatusCode,
};
use rusqlite::params;
use serde::Serialize;
use tower_http::cors::CorsLayer;

use crate::AppState;
use crate::tmux;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health",               get(health))
        .route("/sessions",             get(list_sessions))
        .route("/sessions/:name",       get(get_session))
        .route("/sessions/:name/start", post(start_session))
        .route("/sessions/:name/stop",  post(stop_session))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    host_id: i64,
    sessions_running: i64,
    polled_at: String,
}

#[derive(Serialize)]
struct SessionSummary {
    id: i64,
    name: String,
    project: Option<String>,
    status: String,
    cron_schedule: Option<String>,
    auto_start: bool,
    panes: serde_json::Value,
    polled_at: Option<String>,
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

// ── Handlers ────────────────────────────────────────────────────────────────

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let host_id = state.host_id;
    let running: i64 = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM session_status WHERE status = 'running'",
            [], |r| r.get(0),
        ).unwrap_or(0)
    }).await.unwrap_or(0);

    Json(HealthResponse {
        status: "ok",
        host_id,
        sessions_running: running,
        polled_at: chrono::Utc::now().to_rfc3339(),
    })
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Vec<SessionSummary>> {
    let sessions = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT s.id, s.name, s.project, s.cron_schedule, s.auto_start,
                    COALESCE(ss.status, 'stopped') as status,
                    COALESCE(ss.panes_json, '[]') as panes_json,
                    ss.polled_at
             FROM sessions s
             LEFT JOIN session_status ss ON ss.session_id = s.id
             WHERE s.host_id = ?1 AND s.enabled = 1
             ORDER BY s.project, s.name"
        ).unwrap();

        stmt.query_map(params![state.host_id], |r| {
            let panes_str: String = r.get(6)?;
            Ok(SessionSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                project: r.get(2)?,
                cron_schedule: r.get(3)?,
                auto_start: r.get(4)?,
                status: r.get(5)?,
                panes: serde_json::from_str(&panes_str).unwrap_or(serde_json::json!([])),
                polled_at: r.get(7)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect::<Vec<_>>()
    }).await.unwrap_or_default();

    Json(sessions)
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SessionDetail>, StatusCode> {
    let result = tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();

        let row = db.query_row(
            "SELECT s.id, s.name, s.project, s.cron_schedule, s.auto_start,
                    s.config_json,
                    COALESCE(ss.status, 'stopped'),
                    COALESCE(ss.panes_json, '[]'),
                    ss.polled_at
             FROM sessions s
             LEFT JOIN session_status ss ON ss.session_id = s.id
             WHERE s.name = ?1 AND s.host_id = ?2",
            params![name, state.host_id],
            |r| {
                let panes_str: String = r.get(7)?;
                let config_str: String = r.get(5)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, bool>(4)?,
                    config_str,
                    r.get::<_, String>(6)?,
                    panes_str,
                    r.get::<_, Option<String>>(8)?,
                ))
            },
        ).map_err(|_| StatusCode::NOT_FOUND)?;

        let (id, name, project, cron_schedule, auto_start, config_str, status, panes_str, polled_at) = row;

        let mut estmt = db.prepare(
            "SELECT event, trigger, note, occurred_at
             FROM session_events
             WHERE session_id = ?1
             ORDER BY occurred_at DESC LIMIT 20"
        ).unwrap();

        let events: Vec<EventRow> = estmt.query_map(params![id], |r| {
            Ok(EventRow {
                event: r.get(0)?,
                trigger: r.get(1)?,
                note: r.get(2)?,
                occurred_at: r.get(3)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect();

        Ok::<_, StatusCode>(Json(SessionDetail {
            summary: SessionSummary {
                id,
                name,
                project,
                cron_schedule,
                auto_start,
                status,
                panes: serde_json::from_str(&panes_str).unwrap_or(serde_json::json!([])),
                polled_at,
            },
            config_json: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
            recent_events: events,
        }))
    }).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    result
}

async fn start_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    let state2 = Arc::clone(&state);
    let name2 = name.clone();
    let config_json = tokio::task::spawn_blocking(move || {
        let db = state2.db.lock().unwrap();
        db.query_row(
            "SELECT config_json FROM sessions WHERE name = ?1 AND host_id = ?2",
            params![name2, state2.host_id],
            |r| r.get::<_, String>(0),
        ).map_err(|_| StatusCode::NOT_FOUND)
    }).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    match tmux::start_session(&name, &config_json).await {
        Ok(()) => {
            let name3 = name.clone();
            tokio::task::spawn_blocking(move || {
                let db = state.db.lock().unwrap();
                let id: i64 = db.query_row(
                    "SELECT id FROM sessions WHERE name = ?1 AND host_id = ?2",
                    params![name3, state.host_id], |r| r.get(0),
                ).unwrap();
                let _ = db.execute(
                    "INSERT INTO session_events (session_id, event, trigger) VALUES (?1, 'started', 'manual')",
                    params![id],
                );
            }).await.ok();
            Ok(Json(ActionResponse { ok: true, message: format!("session '{name}' started") }))
        }
        Err(e) => Ok(Json(ActionResponse { ok: false, message: e.to_string() }))
    }
}

async fn stop_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, StatusCode> {
    match tmux::stop_session(&name).await {
        Ok(()) => {
            let name2 = name.clone();
            tokio::task::spawn_blocking(move || {
                let db = state.db.lock().unwrap();
                if let Ok(id) = db.query_row(
                    "SELECT id FROM sessions WHERE name = ?1 AND host_id = ?2",
                    params![name2, state.host_id], |r| r.get::<_, i64>(0),
                ) {
                    let _ = db.execute(
                        "INSERT INTO session_events (session_id, event, trigger) VALUES (?1, 'stopped', 'manual')",
                        params![id],
                    );
                }
            }).await.ok();
            Ok(Json(ActionResponse { ok: true, message: format!("session '{name}' stopped") }))
        }
        Err(e) => Ok(Json(ActionResponse { ok: false, message: e.to_string() }))
    }
}
