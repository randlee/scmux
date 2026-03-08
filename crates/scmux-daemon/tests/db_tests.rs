use scmux_daemon::db::open;
use scmux_daemon::definition_writer;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_db_path(name: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("scmux-{name}-{}-{id}.db", std::process::id()))
}

fn expected_system_hostname() -> String {
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

    for table in &["hosts", "sessions"] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![table],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1, "table '{table}' missing after second open()");
    }

    let index_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_sessions_host'",
            [],
            |r| r.get(0),
        )
        .expect("query index");
    assert_eq!(index_exists, 1);

    let trigger_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='sessions_updated_at'",
            [],
            |r| r.get(0),
        )
        .expect("query trigger");
    assert_eq!(trigger_exists, 1);

    let _ = std::fs::remove_file(path);
}

#[test]
fn td_03_ensure_local_host_uses_system_hostname() {
    let path = temp_db_path("td03");
    let conn = open(path.to_str().expect("utf8 path")).expect("open");
    let id = definition_writer::ensure_local_host(&conn).expect("ensure host");
    let name: String = conn
        .query_row(
            "SELECT name FROM hosts WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .expect("query host name");
    assert_eq!(name, expected_system_hostname());
    let _ = std::fs::remove_file(path);
}

#[test]
fn td_04_ensure_local_host_returns_same_id_on_repeated_calls() {
    let path = temp_db_path("td04");
    let conn = open(path.to_str().expect("utf8 path")).expect("open");
    let id1 = definition_writer::ensure_local_host(&conn).expect("first ensure");
    let id2 = definition_writer::ensure_local_host(&conn).expect("second ensure");
    assert_eq!(id1, id2);
    let _ = std::fs::remove_file(path);
}
