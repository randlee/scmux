# Sprint 1.1 — Foundation (Config + DB)

- Sprint ID: `1.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p1-s1-foundation`
- Base branch: `integrate/phase-1`
- PR target: `integrate/phase-1`

## Context

Current daemon starts with env-only defaults and local-host-only seeding. Schema migration is close but not fully aligned with `docs/schema.sql`. Unit test coverage for config/scheduler window behavior is incomplete.

## Deliverables

1. `crates/scmux-daemon/src/config.rs`
- Add:
  - `pub struct Config { pub daemon: DaemonConfig, pub polling: PollingConfig, pub hosts: Vec<HostConfig> }`
  - `pub struct DaemonConfig { pub port: u16, pub db_path: String, pub default_terminal: String, pub log_level: String }`
  - `pub struct PollingConfig { pub tmux_interval_secs: u64, pub health_interval_secs: u64, pub ci_active_interval_secs: u64, pub ci_idle_interval_secs: u64 }`
  - `pub struct HostConfig { pub name: String, pub address: String, pub ssh_user: Option<String>, pub api_port: u16, pub is_local: bool }`
  - `impl Config { pub fn load() -> anyhow::Result<Self> }`
- Behavior:
  - read `~/.config/scmux/scmux.toml`.
  - if missing, return defaults matching `docs/architecture.md` section 6.

2. `crates/scmux-daemon/src/main.rs`
- Add `config: Config` into `AppState`.
- Load `Config::load()` at startup and use:
  - `config.daemon.db_path` for DB path.
  - `config.daemon.port` for listener port.
  - `config.polling.tmux_interval_secs` and `health_interval_secs` for loop intervals.
- Set default log level to INFO when env is unset.

3. `crates/scmux-daemon/src/db.rs`
- Add `pub fn seed_hosts_from_config(conn: &Connection, hosts: &[HostConfig]) -> Result<()>`.
- Ensure idempotent insert/update semantics for host rows.
- Migration parity updates:
  - add trigger `sessions_updated_at`.
  - use index names from `docs/schema.sql`:
    - `idx_daemon_health_recorded`
    - `idx_session_events_session`

4. `scmux.toml.example`
- Add example config matching architecture section 6.

5. `tests/db_tests.rs`
- Add tests:
  - `T-D-01` `db::open()` creates schema on fresh DB.
  - `T-D-02` `db::open()` is idempotent.
  - `T-D-03` `db::ensure_local_host()` inserts local host if absent.
  - `T-D-04` `db::ensure_local_host()` returns existing local host.

6. `tests/scheduler_tests.rs`
- Make `should_run_now` visible as `pub(crate)`.
- Add tests:
  - `T-D-05` cron fires in 15s window.
  - `T-D-06` cron does not fire in window.
  - `T-D-07` invalid cron returns false.

## Acceptance Criteria

- Daemon loads config from `~/.config/scmux/scmux.toml` with sane defaults when missing.
- `AppState` includes loaded config object.
- Configured hosts are seeded into DB on first run without duplicates.
- DB migration definitions align with `docs/schema.sql` trigger/index names.
- Unit tests T-D-01..T-D-07 pass.

## Requirement IDs Covered

- `DG-04`, `DG-05`, `DG-07`
- `T-D-01`, `T-D-02`, `T-D-03`, `T-D-04`, `T-D-05`, `T-D-06`, `T-D-07`

## Dependencies

- Must be merged before Sprint `1.2`.
