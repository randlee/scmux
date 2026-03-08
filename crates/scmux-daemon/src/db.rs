// DG-02: SQLite is a definition store. Runtime pollers are read-only and update in-memory projection.
use crate::AppState;
use anyhow::{anyhow, bail};
use cron::Schedule;
use rusqlite::{params, types::Value as SqlValue, Connection, OptionalExtension, Result};
use std::sync::Arc;
use std::{fmt::Write as _, str::FromStr};

#[derive(Debug, Clone)]
pub struct NewSession {
    pub name: String,
    pub project: Option<String>,
    pub host_id: i64,
    pub config_json: String,
    pub cron_schedule: Option<String>,
    pub auto_start: bool,
    pub github_repo: Option<String>,
    pub azure_project: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionPatch {
    pub project: Option<Option<String>>,
    pub config_json: Option<String>,
    pub cron_schedule: Option<Option<String>>,
    pub auto_start: Option<bool>,
    pub enabled: Option<bool>,
    pub github_repo: Option<Option<String>>,
    pub azure_project: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct SessionDefinition {
    pub id: i64,
    pub name: String,
    pub project: Option<String>,
    pub host_id: i64,
    pub config_json: String,
    pub cron_schedule: Option<String>,
    pub auto_start: bool,
    pub enabled: bool,
    pub github_repo: Option<String>,
    pub azure_project: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewHost {
    pub name: String,
    pub address: String,
    pub ssh_user: Option<String>,
    pub api_port: u16,
    pub is_local: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HostPatch {
    pub name: Option<String>,
    pub address: Option<String>,
    pub ssh_user: Option<Option<String>>,
    pub api_port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct HostDefinition {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub ssh_user: Option<String>,
    pub api_port: u16,
    pub is_local: bool,
    pub last_seen: Option<String>,
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

pub(crate) fn ensure_local_host(conn: &Connection) -> Result<i64> {
    if let Ok(id) = conn.query_row(
        "SELECT id FROM hosts WHERE is_local = 1 AND enabled = 1 LIMIT 1",
        [],
        |r| r.get(0),
    ) {
        return Ok(id);
    }

    let hostname = system_hostname();
    conn.execute(
        "INSERT INTO hosts (name, address, is_local, enabled) VALUES (?1, 'localhost', 1, 1)",
        params![hostname],
    )?;
    conn.query_row(
        "SELECT id FROM hosts WHERE is_local = 1 AND enabled = 1 LIMIT 1",
        [],
        |r| r.get(0),
    )
}

pub fn session_id(conn: &Connection, host_id: i64, name: &str) -> anyhow::Result<Option<i64>> {
    let value = conn
        .query_row(
            "SELECT id FROM sessions WHERE name = ?1 AND host_id = ?2",
            params![name, host_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(value)
}

pub fn list_sessions_for_host(
    conn: &Connection,
    host_id: i64,
) -> anyhow::Result<Vec<SessionDefinition>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, project, host_id, config_json, cron_schedule, auto_start, enabled, github_repo, azure_project
         FROM sessions
         WHERE host_id = ?1 AND enabled = 1
         ORDER BY host_id, project, name",
    )?;
    let rows = stmt
        .query_map(params![host_id], |r| {
            Ok(SessionDefinition {
                id: r.get(0)?,
                name: r.get(1)?,
                project: r.get(2)?,
                host_id: r.get(3)?,
                config_json: r.get(4)?,
                cron_schedule: r.get(5)?,
                auto_start: r.get(6)?,
                enabled: r.get(7)?,
                github_repo: r.get(8)?,
                azure_project: r.get(9)?,
            })
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    Ok(rows)
}

pub fn get_session_for_host(
    conn: &Connection,
    host_id: i64,
    name: &str,
) -> anyhow::Result<Option<SessionDefinition>> {
    let row = conn
        .query_row(
            "SELECT id, name, project, host_id, config_json, cron_schedule, auto_start, enabled, github_repo, azure_project
             FROM sessions
             WHERE host_id = ?1 AND name = ?2 AND enabled = 1
             LIMIT 1",
            params![host_id, name],
            |r| {
                Ok(SessionDefinition {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    project: r.get(2)?,
                    host_id: r.get(3)?,
                    config_json: r.get(4)?,
                    cron_schedule: r.get(5)?,
                    auto_start: r.get(6)?,
                    enabled: r.get(7)?,
                    github_repo: r.get(8)?,
                    azure_project: r.get(9)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn list_hosts(conn: &Connection) -> anyhow::Result<Vec<HostDefinition>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, address, ssh_user, api_port, is_local, last_seen
         FROM hosts
         WHERE enabled = 1
         ORDER BY is_local DESC, name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(HostDefinition {
                id: r.get(0)?,
                name: r.get(1)?,
                address: r.get(2)?,
                ssh_user: r.get(3)?,
                api_port: r.get(4)?,
                is_local: r.get(5)?,
                last_seen: r.get(6)?,
            })
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    Ok(rows)
}

pub fn get_host(conn: &Connection, host_id: i64) -> anyhow::Result<Option<HostDefinition>> {
    let row = conn
        .query_row(
            "SELECT id, name, address, ssh_user, api_port, is_local, last_seen
             FROM hosts
             WHERE id = ?1 AND enabled = 1
             LIMIT 1",
            params![host_id],
            |r| {
                Ok(HostDefinition {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    address: r.get(2)?,
                    ssh_user: r.get(3)?,
                    api_port: r.get(4)?,
                    is_local: r.get(5)?,
                    last_seen: r.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub(crate) fn create_session(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    session: &NewSession,
) -> anyhow::Result<i64> {
    validate_config_session_name(&session.name, &session.config_json)?;
    if let Some(expr) = &session.cron_schedule {
        validate_cron(expr)?;
    }

    conn.execute(
        "INSERT INTO sessions (
            name, project, host_id, config_json, cron_schedule, auto_start, enabled, github_repo, azure_project
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
        params![
            session.name,
            session.project,
            session.host_id,
            session.config_json,
            session.cron_schedule,
            session.auto_start,
            session.github_repo,
            session.azure_project
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn update_session(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    host_id: i64,
    name: &str,
    patch: &SessionPatch,
) -> anyhow::Result<bool> {
    let session_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM sessions WHERE name = ?1 AND host_id = ?2 AND enabled = 1",
            params![name, host_id],
            |r| r.get(0),
        )
        .optional()?;
    if session_id.is_none() {
        return Ok(false);
    }

    if let Some(config_json) = &patch.config_json {
        validate_config_session_name(name, config_json)?;
    }
    if let Some(Some(expr)) = &patch.cron_schedule {
        validate_cron(expr)?;
    }

    let mut set_parts: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(project) = &patch.project {
        set_parts.push("project = ?");
        values.push(match project {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if let Some(config_json) = &patch.config_json {
        set_parts.push("config_json = ?");
        values.push(SqlValue::Text(config_json.clone()));
    }
    if let Some(cron_schedule) = &patch.cron_schedule {
        set_parts.push("cron_schedule = ?");
        values.push(match cron_schedule {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if let Some(auto_start) = patch.auto_start {
        set_parts.push("auto_start = ?");
        values.push(SqlValue::Integer(if auto_start { 1 } else { 0 }));
    }
    if let Some(enabled) = patch.enabled {
        set_parts.push("enabled = ?");
        values.push(SqlValue::Integer(if enabled { 1 } else { 0 }));
    }
    if let Some(github_repo) = &patch.github_repo {
        set_parts.push("github_repo = ?");
        values.push(match github_repo {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if let Some(azure_project) = &patch.azure_project {
        set_parts.push("azure_project = ?");
        values.push(match azure_project {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }

    if set_parts.is_empty() {
        return Ok(true);
    }

    let mut sql = String::from("UPDATE sessions SET ");
    for (idx, part) in set_parts.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(part);
    }
    let _ = write!(&mut sql, " WHERE name = ? AND host_id = ? AND enabled = 1");
    values.push(SqlValue::Text(name.to_string()));
    values.push(SqlValue::Integer(host_id));

    conn.execute(&sql, rusqlite::params_from_iter(values))?;
    Ok(true)
}

pub(crate) fn soft_delete_session(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    host_id: i64,
    name: &str,
) -> anyhow::Result<bool> {
    let changed = conn.execute(
        "UPDATE sessions SET enabled = 0 WHERE name = ?1 AND host_id = ?2 AND enabled = 1",
        params![name, host_id],
    )?;
    Ok(changed > 0)
}

pub(crate) fn create_host(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    host: &NewHost,
) -> anyhow::Result<i64> {
    conn.execute(
        "INSERT INTO hosts (name, address, ssh_user, api_port, is_local, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![
            host.name,
            host.address,
            host.ssh_user,
            host.api_port,
            host.is_local
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn update_host(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    host_id: i64,
    patch: &HostPatch,
) -> anyhow::Result<bool> {
    let row_exists = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM hosts WHERE id = ?1 AND enabled = 1",
            params![host_id],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !row_exists {
        return Ok(false);
    }

    let mut set_parts: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(name) = &patch.name {
        set_parts.push("name = ?");
        values.push(SqlValue::Text(name.clone()));
    }
    if let Some(address) = &patch.address {
        set_parts.push("address = ?");
        values.push(SqlValue::Text(address.clone()));
    }
    if let Some(ssh_user) = &patch.ssh_user {
        set_parts.push("ssh_user = ?");
        values.push(match ssh_user {
            Some(value) => SqlValue::Text(value.clone()),
            None => SqlValue::Null,
        });
    }
    if let Some(api_port) = patch.api_port {
        set_parts.push("api_port = ?");
        values.push(SqlValue::Integer(api_port as i64));
    }

    if set_parts.is_empty() {
        return Ok(true);
    }

    let mut sql = String::from("UPDATE hosts SET ");
    for (idx, part) in set_parts.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(part);
    }
    sql.push_str(" WHERE id = ? AND enabled = 1");
    values.push(SqlValue::Integer(host_id));

    conn.execute(&sql, rusqlite::params_from_iter(values))?;
    Ok(true)
}

pub(crate) fn soft_delete_host(
    _guard: &crate::definition_writer::WriteGuard,
    conn: &Connection,
    host_id: i64,
) -> anyhow::Result<bool> {
    let changed = conn.execute(
        "UPDATE hosts SET enabled = 0 WHERE id = ?1 AND enabled = 1 AND is_local = 0",
        params![host_id],
    )?;
    Ok(changed > 0)
}

pub(crate) async fn write_health(state: &Arc<AppState>) -> anyhow::Result<()> {
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let running = {
            let runtime = state.runtime.lock().expect("runtime lock");
            runtime.live_session_count()
        };

        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO daemon_health (host_id, status, sessions_running) VALUES (?1, 'ok', ?2)",
            params![state.host_id, running],
        )?;

        db.execute(
            "DELETE FROM daemon_health WHERE recorded_at < datetime('now', '-7 days')",
            [],
        )?;

        Ok::<_, anyhow::Error>(())
    })
    .await??;

    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hosts (
            id         INTEGER PRIMARY KEY,
            name       TEXT    NOT NULL UNIQUE,
            address    TEXT    NOT NULL,
            ssh_user   TEXT,
            api_port   INTEGER NOT NULL DEFAULT 7878,
            is_local   BOOLEAN NOT NULL DEFAULT 0,
            enabled    BOOLEAN NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT (datetime('now')),
            last_seen  TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id            INTEGER PRIMARY KEY,
            name          TEXT    NOT NULL,
            project       TEXT,
            host_id       INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
            config_json   TEXT    NOT NULL,
            cron_schedule TEXT,
            auto_start    BOOLEAN NOT NULL DEFAULT 0,
            enabled       BOOLEAN NOT NULL DEFAULT 1,
            github_repo   TEXT,
            azure_project TEXT,
            created_at    DATETIME NOT NULL DEFAULT (datetime('now')),
            updated_at    DATETIME NOT NULL DEFAULT (datetime('now')),
            UNIQUE (name, host_id)
        );

        CREATE TABLE IF NOT EXISTS session_status (
            session_id  INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
            status      TEXT    NOT NULL DEFAULT 'stopped',
            panes_json  TEXT,
            polled_at   DATETIME NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS session_events (
            id         INTEGER PRIMARY KEY,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            event      TEXT    NOT NULL,
            trigger    TEXT    NOT NULL,
            note       TEXT,
            occurred_at DATETIME NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS daemon_health (
            id               INTEGER PRIMARY KEY,
            host_id          INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
            status           TEXT    NOT NULL,
            sessions_running INTEGER,
            note             TEXT,
            recorded_at      DATETIME NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS session_ci (
            id            INTEGER PRIMARY KEY,
            session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            provider      TEXT    NOT NULL,
            status        TEXT    NOT NULL,
            data_json     TEXT,
            tool_message  TEXT,
            polled_at     DATETIME,
            next_poll_at  DATETIME,
            UNIQUE (session_id, provider)
        );

        CREATE INDEX IF NOT EXISTS idx_session_ci_session  ON session_ci (session_id);
        CREATE INDEX IF NOT EXISTS idx_session_ci_next_poll ON session_ci (next_poll_at);

        CREATE TABLE IF NOT EXISTS session_atm (
            session_name    TEXT PRIMARY KEY,
            agent_id        TEXT,
            team            TEXT,
            state           TEXT NOT NULL DEFAULT 'unknown',
            last_transition TEXT,
            updated_at      TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_session_atm_session_name ON session_atm (session_name);

        CREATE INDEX IF NOT EXISTS idx_sessions_host    ON sessions (host_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions (project);
        CREATE INDEX IF NOT EXISTS idx_session_events_session ON session_events (session_id, occurred_at);
        CREATE INDEX IF NOT EXISTS idx_daemon_health_recorded ON daemon_health (recorded_at);

        CREATE TRIGGER IF NOT EXISTS sessions_updated_at
          AFTER UPDATE ON sessions
          FOR EACH ROW
          BEGIN
            UPDATE sessions SET updated_at = datetime('now') WHERE id = OLD.id;
          END;
    "#,
    )?;
    ensure_hosts_last_seen_column(conn)?;
    ensure_hosts_enabled_column(conn)?;
    Ok(())
}

fn ensure_hosts_last_seen_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(hosts)")?;
    let has_last_seen = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "last_seen");

    if !has_last_seen {
        conn.execute("ALTER TABLE hosts ADD COLUMN last_seen TEXT", [])?;
    }
    Ok(())
}

fn ensure_hosts_enabled_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(hosts)")?;
    let has_enabled = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "enabled");

    if !has_enabled {
        conn.execute(
            "ALTER TABLE hosts ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT 1",
            [],
        )?;
    }
    Ok(())
}

fn validate_config_session_name(name: &str, config_json: &str) -> anyhow::Result<()> {
    let value: serde_json::Value =
        serde_json::from_str(config_json).map_err(|e| anyhow!("invalid config_json JSON: {e}"))?;
    let json_session_name = value
        .get("session_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("config_json.session_name is required"))?;
    if json_session_name != name {
        bail!("config_json.session_name must equal session name");
    }
    Ok(())
}

fn validate_cron(expr: &str) -> anyhow::Result<()> {
    let normalized = normalize_cron_expr(expr);
    Schedule::from_str(&normalized).map_err(|e| anyhow!("invalid cron_schedule: {e}"))?;
    Ok(())
}

fn normalize_cron_expr(expr: &str) -> String {
    if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    }
}

fn system_hostname() -> String {
    if let Ok(output) = std::process::Command::new("hostname").output() {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
    }

    std::env::var("HOSTNAME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "local".to_string())
}
