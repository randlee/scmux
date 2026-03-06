# Sprint 2.1 — Multi-Host Reachability

- Sprint ID: `2.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s1-multihost`
- Base branch: `integrate/phase-2`
- PR target: `integrate/phase-2`

## Context

Host definitions are seeded by `seed_hosts_from_config()` into the `hosts` table on startup. No active
reachability loop or remote session fetching is implemented. `/hosts` returns static DB rows without
`reachable` or `last_seen` fields. Dashboard only sees local sessions.

## Architecture

**Dashboard → local daemon only.** The dashboard never contacts remote hosts directly. The local daemon
is responsible for:
1. Probing remote host reachability via `ping`
2. Fetching session data from remote daemons via `GET remote:api_port/sessions`
3. Caching remote session data locally so it's available when a host goes offline

This enables the core use case: work on a remote machine, disconnect, resume later from any device.

## AppState Extension

```rust
pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub config: Config,
    pub reachability: std::sync::Mutex<std::collections::HashMap<i64, hosts::HostReachability>>,
}
```

`HostReachability` tracks consecutive failure count for debounce (see below).

## Probe Mechanism

Single background task pings hosts **sequentially** (no concurrent ping threads):

```rust
async fn probe_host(address: &str) -> bool {
    let args = if cfg!(target_os = "macos") {
        vec!["-c", "1", "-t", "10", address]
    } else {
        vec!["-c", "1", "-W", "10", address]
    };
    tokio::process::Command::new("ping")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}
```

- Timeout: 10 seconds per host.
- `is_local = true` hosts: always reachable, skip probe.
- **Design rationale**: Remote hosts are only reachable via VPN or on-site. Ping failure = VPN down = expected condition, not an error (MH-03).

## Reachability State Machine (debounce)

To prevent status bouncing on transient VPN blips:

```rust
pub struct HostReachability {
    pub host_id: i64,
    pub reachable: bool,
    pub last_seen: Option<String>,       // ISO 8601 UTC, updated on successful probe
    pub consecutive_failures: u32,        // reset to 0 on any success
}
```

- **Declare unreachable**: after **3 consecutive ping failures**. Do not flip on 1 or 2 failures.
- **Resume reachable**: on **1 successful ping**. Recovery is immediate.
- Between transitions: maintain current `reachable` status unchanged.

## Adaptive Poll Frequency

- **Active** (any API request in last 60s AND host has live sessions): probe every `health_interval_secs` (default 30s).
- **Idle** (no recent API requests OR no live sessions on host): probe every `health_interval_secs * 10` (default 5 min).
- Track `last_api_access: Instant` in `AppState`; update on every inbound API request via middleware or handler.

This keeps the daemon quiet when nobody is watching and responsive when the dashboard is open.

## Remote Session Fetching

When a host is **reachable**, the poll loop also fetches its sessions via HTTP:

```
GET http://{address}:{api_port}/sessions
```

Store fetched sessions in the local DB tagged with `host_id`. This provides last-known data when the
host later becomes unreachable (MH-04).

Use `reqwest` (already in workspace deps) with a 5-second timeout. Fetch failures are logged at WARN
and do not affect the reachability state (ping and fetch are independent).

## Deliverables

### 1. `crates/scmux-daemon/src/hosts.rs` (new)

- `HostReachability` struct (with `consecutive_failures` field).
- `probe_host(address: &str) -> bool` — sequential ping.
- `fetch_remote_sessions(address: &str, port: u16) -> anyhow::Result<Vec<Session>>` — HTTP GET /sessions from remote daemon.
- `poll_hosts(state: Arc<AppState>) -> anyhow::Result<()>` — main poll loop body:
  - Load all hosts from DB.
  - For each remote host, sequentially: probe → update reachability map with debounce → if reachable, fetch sessions → upsert into local DB.
  - Update `state.reachability`.
  - Log at DEBUG per host; WARN on fetch failure.

### 2. `crates/scmux-daemon/src/main.rs`

- Add `last_api_access: std::sync::Mutex<std::time::Instant>` to `AppState`.
- Update `last_api_access` on every inbound request (middleware or per-handler).
- Spawn `host_poll_loop` — single task, sequential hosts, adaptive interval.

### 3. `crates/scmux-daemon/src/api.rs`

`GET /hosts` per host:
```json
{
  "id": 1, "name": "spark", "address": "spark.local",
  "api_port": 7878, "is_local": false,
  "reachable": false,
  "last_seen": "2026-03-06T03:00:00Z"
}
```

`GET /dashboard-config.json`:
```json
{
  "hosts": [
    { "name": "local", "url": "http://localhost:7878", "is_local": true },
    { "name": "spark", "url": "http://spark.local:7878", "is_local": false }
  ],
  "default_terminal": "iterm2",
  "poll_interval_ms": 15000
}
```
- `poll_interval_ms`: from `config.polling.tmux_interval_secs * 1000` (dashboard session refresh rate).
- `default_terminal`: from `config.daemon.default_terminal`.
- `is_local` per host: required for dashboard to route jump POSTs correctly.

### 4. `crates/scmux-daemon/src/db.rs`

- Add `last_seen TEXT` column to `hosts` table in migration.
- `update_host_last_seen(conn, host_id, last_seen)` helper.
- `upsert_remote_session(conn, host_id, session)` — insert or update session from remote host.

### 5. Tests

- **T-I-10**: `poll_hosts` with unreachable address (`192.0.2.1`, RFC 5737 TEST-NET). Returns `Ok(())`.
- **T-I-11**: After 3 simulated failures on a host entry, verify `reachable == false` in reachability map.
- **T-I-12**: After unreachable state, one simulated success → verify `reachable == true`, `consecutive_failures == 0`.

Tests must not require network access. Simulate probe results by injecting a mock or by directly manipulating reachability map state.

## Acceptance Criteria

- `GET /hosts` includes `reachable`, `last_seen`, `is_local` per host.
- Local hosts always `reachable: true`.
- Unreachable hosts do not crash daemon or produce error logs (WARN only).
- Status does not flip on fewer than 3 consecutive failures.
- Recovery is immediate (1 success).
- Remote sessions fetched and stored locally when host reachable.
- Last-known session data served from local DB when host unreachable (MH-04).
- `dashboard-config.json` includes `is_local`, `default_terminal`, `poll_interval_ms`.
- No error dialogs propagated — all host failures handled gracefully (MH-09).
- T-I-10..T-I-12 pass without network access.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test --workspace` passes.

## Requirement IDs Covered

- `MH-01..MH-05`, `MH-09` (MH-06..08 are dashboard rendering, covered in S2.2)
- `API-14`, `API-15`
- `T-I-10`, `T-I-11`, `T-I-12`

## Dependencies

- Requires `integrate/phase-1` merged into `integrate/phase-2` ✅ (done).
- Must merge before Sprint `2.2`.
