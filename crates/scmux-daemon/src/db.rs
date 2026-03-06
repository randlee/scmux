use crate::config::RemoteHost;
use crate::AppState;
use rusqlite::{params, Connection, Result};
use std::sync::Arc;

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

pub fn seed_remote_hosts(conn: &Connection, hosts: &[RemoteHost]) -> anyhow::Result<()> {
    for host in hosts {
        conn.execute(
            "INSERT OR IGNORE INTO hosts (name, address, ssh_user, api_port, is_local)
             VALUES (?1, ?2, ?3, 7700, 0)",
            params![host.name, host.hostname, host.ssh_user],
        )?;
    }
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
        CREATE INDEX IF NOT EXISTS idx_session_events_session ON session_events (session_id, occurred_at);
        CREATE INDEX IF NOT EXISTS idx_daemon_health_recorded ON daemon_health (recorded_at);

        CREATE TRIGGER IF NOT EXISTS sessions_updated_at
          AFTER UPDATE ON sessions
          FOR EACH ROW
          BEGIN
            UPDATE sessions SET updated_at = datetime('now') WHERE id = OLD.id;
          END;
    "#)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(name: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("scmux-{name}-{}-{id}.db", std::process::id()))
    }

    #[test]
    fn td_01_open_creates_schema_on_fresh_db() {
        let path = temp_db_path("td01");
        let conn = open(path.to_str().expect("utf8 path")).expect("open fresh db");
        let table_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='hosts'",
                [],
                |r| r.get(0),
            )
            .expect("query table");
        assert_eq!(table_exists, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn td_02_open_is_idempotent_on_existing_db() {
        let path = temp_db_path("td02");
        let _ = open(path.to_str().expect("utf8 path")).expect("first open");
        let conn = open(path.to_str().expect("utf8 path")).expect("second open");
        let index_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_sessions_host'",
                [],
                |r| r.get(0),
            )
            .expect("query index");
        assert_eq!(index_exists, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn td_03_ensure_local_host_returns_same_id_on_repeated_calls() {
        let path = temp_db_path("td03");
        let conn = open(path.to_str().expect("utf8 path")).expect("open");
        let id1 = ensure_local_host(&conn).expect("first ensure");
        let id2 = ensure_local_host(&conn).expect("second ensure");
        assert_eq!(id1, id2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn td_04_ensure_local_host_uses_system_hostname() {
        let path = temp_db_path("td04");
        let conn = open(path.to_str().expect("utf8 path")).expect("open");
        let id = ensure_local_host(&conn).expect("ensure host");
        let name: String = conn
            .query_row("SELECT name FROM hosts WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .expect("query host name");
        assert_eq!(name, system_hostname());
        let _ = std::fs::remove_file(path);
    }
}
