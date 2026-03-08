use crate::db;
use crate::AppState;
use rusqlite::Connection;
use std::sync::Arc;

pub struct WriteGuard(());

#[derive(Debug)]
pub enum WriteError {
    NotFound,
    Conflict(String),
    Validation(String),
    Forbidden(String),
    Internal(String),
}

impl WriteError {
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => "not found".to_string(),
            Self::Conflict(msg)
            | Self::Validation(msg)
            | Self::Forbidden(msg)
            | Self::Internal(msg) => msg.clone(),
        }
    }
}

pub fn create_session(conn: &Connection, new_session: &db::NewSession) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    validate_approved_project(&new_session.name, &new_session.config_json)?;
    db::create_session(&guard, conn, new_session).map_err(map_write_error)
}

pub fn patch_session(
    conn: &Connection,
    host_id: i64,
    name: &str,
    patch: &db::SessionPatch,
) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    if let Some(config_json) = patch.config_json.as_ref() {
        validate_approved_project(name, config_json)?;
    } else if patch.enabled == Some(true) {
        let current = db::get_session_for_host(conn, host_id, name)
            .map_err(map_write_error)?
            .ok_or(WriteError::NotFound)?;
        validate_approved_project(name, &current.config_json)?;
    }

    db::update_session(&guard, conn, host_id, name, patch).map_err(map_write_error)
}

pub fn delete_session(conn: &Connection, host_id: i64, name: &str) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    db::soft_delete_session(&guard, conn, host_id, name).map_err(map_write_error)
}

pub fn create_host(conn: &Connection, host: &db::NewHost) -> Result<i64, WriteError> {
    let guard = WriteGuard(());
    db::create_host(&guard, conn, host).map_err(map_write_error)
}

pub fn patch_host(
    conn: &Connection,
    host_id: i64,
    patch: &db::HostPatch,
) -> Result<bool, WriteError> {
    let guard = WriteGuard(());
    db::update_host(&guard, conn, host_id, patch).map_err(map_write_error)
}

pub fn delete_host(conn: &Connection, host_id: i64) -> Result<bool, WriteError> {
    let Some(row) = db::get_host(conn, host_id).map_err(map_write_error)? else {
        return Err(WriteError::NotFound);
    };
    if row.is_local {
        return Err(WriteError::Forbidden(
            "local host definition cannot be deleted".to_string(),
        ));
    }
    let guard = WriteGuard(());
    db::soft_delete_host(&guard, conn, host_id).map_err(map_write_error)
}

pub fn ensure_local_host(conn: &Connection) -> Result<i64, WriteError> {
    db::ensure_local_host(conn).map_err(|err| WriteError::Internal(err.to_string()))
}

pub async fn write_health(state: &Arc<AppState>) -> Result<(), WriteError> {
    db::write_health(state).await.map_err(map_write_error)
}

fn validate_approved_project(session_name: &str, config_json: &str) -> Result<(), WriteError> {
    let value: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|err| WriteError::Validation(format!("invalid config_json JSON: {err}")))?;

    let json_session_name = value
        .get("session_name")
        .and_then(|raw| raw.as_str())
        .ok_or_else(|| {
            WriteError::Validation("config_json.session_name is required".to_string())
        })?;
    if json_session_name != session_name {
        return Err(WriteError::Validation(
            "config_json.session_name must equal session name".to_string(),
        ));
    }

    let panes = value
        .get("panes")
        .and_then(|raw| raw.as_array())
        .ok_or_else(|| {
            WriteError::Validation(
                "config_json.panes[] is required for approved projects".to_string(),
            )
        })?;

    if panes.is_empty() {
        return Err(WriteError::Validation(
            "config_json.panes[] must contain at least one pane".to_string(),
        ));
    }

    Ok(())
}

fn map_write_error(err: anyhow::Error) -> WriteError {
    let message = err.to_string();
    if message.contains("UNIQUE constraint failed") {
        return WriteError::Conflict(message);
    }
    if message.contains("invalid") || message.contains("required") || message.contains("must") {
        return WriteError::Validation(message);
    }
    WriteError::Internal(message)
}
