// DG-02: This module is the sole SQLite writer. All Connection access is via AppState.db mutex.
// No other module calls Connection::open — verified by audit (grep Connection::open crates/).
use crate::config::HostConfig;
use crate::AppState;
use anyhow::{anyhow, bail};
use cron::Schedule;
use rusqlite::{params, Connection, Result};
use rusqlite::{types::Value as SqlValue, OptionalExtension};
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
pub struct RemoteSessionUpsert {
    pub name: String,
    pub project: Option<String>,
    pub cron_schedule: Option<String>,
    pub auto_start: bool,
    pub status: String,
    pub panes_json: String,
    pub polled_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CiSession {
    pub id: i64,
    pub name: String,
    pub github_repo: Option<String>,
    pub azure_project: Option<String>,
    pub has_active_pane: bool,
}

#[derive(Debug, Clone)]
pub struct SessionCiUpdate {
    pub session_id: i64,
    pub provider: String,
    pub status: String,
    pub data_json: Option<String>,
    pub tool_message: Option<String>,
    pub polled_at: String,
    pub next_poll_at: String,
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn ensure_local_host(conn: &Connection) -> Result<i64> {
    if let Ok(id) = conn.query_row("SELECT id FROM hosts WHERE is_local = 1 LIMIT 1", [], |r| {
        r.get(0)
    }) {
        return Ok(id);
    }

    let hostname = system_hostname();
    conn.execute(
        "INSERT INTO hosts (name, address, is_local) VALUES (?1, 'localhost', 1)",
        params![hostname],
    )?;
    conn.query_row("SELECT id FROM hosts WHERE is_local = 1 LIMIT 1", [], |r| {
        r.get(0)
    })
}

pub fn seed_hosts_from_config(conn: &Connection, hosts: &[HostConfig]) -> anyhow::Result<()> {
    for host in hosts {
        conn.execute(
            "INSERT OR IGNORE INTO hosts (name, address, ssh_user, api_port, is_local)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                host.name,
                host.address,
                host.ssh_user,
                host.api_port.unwrap_or(7700),
                host.is_local.unwrap_or(false)
            ],
        )?;
    }
    Ok(())
}

pub fn update_host_last_seen(
    conn: &Connection,
    host_id: i64,
    last_seen: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE hosts SET last_seen = ?1 WHERE id = ?2",
        params![last_seen, host_id],
    )?;
    Ok(())
}

pub fn create_session(conn: &Connection, session: &NewSession) -> anyhow::Result<i64> {
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

pub fn update_session(
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

pub fn soft_delete_session(conn: &Connection, host_id: i64, name: &str) -> anyhow::Result<bool> {
    let changed = conn.execute(
        "UPDATE sessions SET enabled = 0 WHERE name = ?1 AND host_id = ?2 AND enabled = 1",
        params![name, host_id],
    )?;
    Ok(changed > 0)
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

pub fn upsert_remote_session(
    conn: &Connection,
    host_id: i64,
    session: &RemoteSessionUpsert,
) -> anyhow::Result<()> {
    let config_json = serde_json::json!({ "session_name": session.name }).to_string();
    conn.execute(
        "INSERT INTO sessions (
            name, project, host_id, config_json, cron_schedule, auto_start, enabled
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
         ON CONFLICT(name, host_id) DO UPDATE SET
            project = excluded.project,
            cron_schedule = excluded.cron_schedule,
            auto_start = excluded.auto_start,
            enabled = 1",
        params![
            session.name,
            session.project,
            host_id,
            config_json,
            session.cron_schedule,
            session.auto_start
        ],
    )?;

    let session_id: i64 = conn.query_row(
        "SELECT id FROM sessions WHERE name = ?1 AND host_id = ?2",
        params![session.name, host_id],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT INTO session_status (session_id, status, panes_json, polled_at)
         VALUES (
            ?1,
            ?2,
            ?3,
            COALESCE(?4, datetime('now'))
         )
         ON CONFLICT(session_id) DO UPDATE SET
            status = excluded.status,
            panes_json = excluded.panes_json,
            polled_at = excluded.polled_at",
        params![
            session_id,
            session.status,
            session.panes_json,
            session.polled_at
        ],
    )?;

    Ok(())
}

pub fn list_ci_sessions(conn: &Connection) -> anyhow::Result<Vec<CiSession>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.github_repo, s.azure_project, COALESCE(ss.panes_json, '[]')
         FROM sessions s
         LEFT JOIN session_status ss ON ss.session_id = s.id
         WHERE s.enabled = 1
           AND (s.github_repo IS NOT NULL OR s.azure_project IS NOT NULL)
         ORDER BY s.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let panes_json: String = r.get(4)?;
            Ok(CiSession {
                id: r.get(0)?,
                name: r.get(1)?,
                github_repo: r.get(2)?,
                azure_project: r.get(3)?,
                has_active_pane: panes_json_has_active(&panes_json),
            })
        })?
        .filter_map(|row| row.ok())
        .collect::<Vec<_>>();
    Ok(rows)
}

pub fn ci_provider_due(
    conn: &Connection,
    session_id: i64,
    provider: &str,
    now_iso: &str,
) -> anyhow::Result<bool> {
    let due = conn
        .query_row(
            "SELECT next_poll_at <= ?3
             FROM session_ci
             WHERE session_id = ?1 AND provider = ?2",
            params![session_id, provider, now_iso],
            |r| r.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(true);
    Ok(due)
}

pub fn upsert_session_ci(conn: &Connection, update: &SessionCiUpdate) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO session_ci (
            session_id, provider, status, data_json, tool_message, polled_at, next_poll_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(session_id, provider) DO UPDATE SET
            status = excluded.status,
            data_json = excluded.data_json,
            tool_message = excluded.tool_message,
            polled_at = excluded.polled_at,
            next_poll_at = excluded.next_poll_at",
        params![
            update.session_id,
            update.provider,
            update.status,
            update.data_json,
            update.tool_message,
            update.polled_at,
            update.next_poll_at
        ],
    )?;
    Ok(())
}

pub fn log_session_event(
    conn: &Connection,
    session_id: i64,
    event: &str,
    trigger: &str,
    note: Option<&str>,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO session_events (session_id, event, trigger, note) VALUES (?1, ?2, ?3, ?4)",
        params![session_id, event, trigger, note],
    )?;
    Ok(())
}

pub async fn write_health(state: &Arc<AppState>) -> anyhow::Result<()> {
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        let running: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM session_status WHERE status = 'running'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        db.execute(
            "INSERT INTO daemon_health (host_id, status, sessions_running) VALUES (?1, 'ok', ?2)",
            params![state.host_id, running],
        )?;

        // Prune records older than 7 days
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
    // DG-06: Migration is idempotent — safe to run on every startup.
    // All DDL statements use CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS /
    // CREATE TRIGGER IF NOT EXISTS so re-running on an existing schema is a no-op.
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS hosts (
            id         INTEGER PRIMARY KEY,
            name       TEXT    NOT NULL UNIQUE,
            address    TEXT    NOT NULL,
            ssh_user   TEXT,
            api_port   INTEGER NOT NULL DEFAULT 7700,
            is_local   BOOLEAN NOT NULL DEFAULT 0,
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
    "#)?;
    ensure_hosts_last_seen_column(conn)?;
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

fn panes_json_has_active(panes_json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(panes_json) else {
        return false;
    };
    let Some(items) = value.as_array() else {
        return false;
    };
    items.iter().any(|item| {
        item.get("status")
            .and_then(|status| status.as_str())
            .is_some_and(|status| status.eq_ignore_ascii_case("active"))
    })
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
