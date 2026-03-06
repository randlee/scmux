use rusqlite::{Connection, Result, params};
use std::sync::Arc;
use crate::AppState;

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn ensure_local_host(conn: &Connection) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO hosts (name, address, is_local) VALUES ('local', 'localhost', 1)",
        [],
    )?;
    conn.query_row(
        "SELECT id FROM hosts WHERE is_local = 1 LIMIT 1",
        [],
        |r| r.get(0),
    )
}

pub async fn write_health(state: &Arc<AppState>) -> anyhow::Result<()> {
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let db = state.db.lock().unwrap();
        let running: i64 = db.query_row(
            "SELECT COUNT(*) FROM session_status WHERE status = 'running'",
            [],
            |r| r.get(0),
        ).unwrap_or(0);

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
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS hosts (
            id         INTEGER PRIMARY KEY,
            name       TEXT    NOT NULL UNIQUE,
            address    TEXT    NOT NULL,
            ssh_user   TEXT,
            api_port   INTEGER NOT NULL DEFAULT 7700,
            is_local   BOOLEAN NOT NULL DEFAULT 0,
            created_at DATETIME NOT NULL DEFAULT (datetime('now')),
            last_seen  DATETIME
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
        CREATE INDEX IF NOT EXISTS idx_events_session   ON session_events (session_id, occurred_at);
        CREATE INDEX IF NOT EXISTS idx_health_recorded  ON daemon_health (recorded_at);
    "#)
}
