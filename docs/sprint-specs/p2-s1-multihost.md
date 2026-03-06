# Sprint 2.1 — Multi-Host Reachability

- Sprint ID: `2.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s1-multihost`
- Base branch: `integrate/phase-2`
- PR target: `integrate/phase-2`

## Context

Host definitions are seeded by `seed_hosts_from_config()` into the `hosts` table on startup. No active
reachability loop or stale-data handling is implemented. `/hosts` currently returns static DB rows without
`reachable` or `last_seen` fields.

## AppState Extension

`AppState` must carry an in-memory reachability map. Add to `crates/scmux-daemon/src/main.rs`:

```rust
pub struct AppState {
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub config: Config,
    pub reachability: std::sync::Mutex<std::collections::HashMap<i64, hosts::HostReachability>>,
}
```

## Probe Mechanism

Reachability is determined by running the system `ping` command against each host's `address`.

**Design rationale**: Remote hosts are only reachable when on VPN or on-site. A failed ping cleanly
represents "host unreachable right now" — no VPN, no ping. This is the expected operating condition
(MH-03: unreachable is not an error). Future enhancement (not this sprint): hosts that are reachable
via network but don't respond to ping could fall back to TCP connect. Not needed now.


- macOS: `ping -c 1 -t 2 <address>`
- Linux: `ping -c 1 -W 2 <address>`
- Exit code 0 = reachable; non-zero = unreachable.
- `is_local = true` hosts are always considered reachable — skip probe.

```rust
async fn probe_host(address: &str) -> bool {
    let args = if cfg!(target_os = "macos") {
        vec!["-c", "1", "-t", "2", address]
    } else {
        vec!["-c", "1", "-W", "2", address]
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

Add `tokio` process feature to workspace deps if not already present: `tokio = { version = "1", features = ["full"] }` (already included).

## Deliverables

### 1. `crates/scmux-daemon/src/hosts.rs` (new)

```rust
pub struct HostReachability {
    pub host_id: i64,
    pub reachable: bool,
    pub last_seen: Option<String>,  // ISO 8601 UTC, set on successful probe
}

pub async fn poll_hosts(state: Arc<AppState>) -> anyhow::Result<()>
```

- Load all hosts from DB inside `spawn_blocking`.
- Skip `is_local = true` hosts (always reachable; initialize map entry with `reachable: true`).
- Probe each remote host via `probe_host()`.
- On success: `reachable = true`, set `last_seen` = UTC now (chrono `Utc::now().to_rfc3339()`).
- On failure: `reachable = false`, preserve existing `last_seen` from prior map entry.
- Persist `last_seen` to `hosts.last_seen` column via `spawn_blocking` on successful probes only.
- Write updated entries into `state.reachability` under the mutex.
- Log each probe result at DEBUG; log failures at WARN. Never return Err for individual probe failures.

### 2. `crates/scmux-daemon/src/main.rs`

- Initialize `AppState.reachability = Mutex::new(HashMap::new())`.
- Spawn `host_poll_loop`:
  ```rust
  tokio::spawn(async move {
      loop {
          if let Err(e) = hosts::poll_hosts(Arc::clone(&state)).await {
              tracing::warn!(error = %e, "host poll cycle failed");
          }
          tokio::time::sleep(Duration::from_secs(interval_secs)).await;
      }
  });
  ```
- Interval: `config.polling.health_interval_secs` (default: 30).

### 3. `crates/scmux-daemon/src/api.rs`

`GET /hosts` response per host:
```json
{
  "id": 1,
  "name": "local",
  "address": "localhost",
  "api_port": 7878,
  "is_local": true,
  "reachable": true,
  "last_seen": "2026-03-06T04:00:00Z"
}
```
- `reachable`: from `state.reachability`; default `true` if host not yet probed.
- `last_seen`: from reachability map; `null` if never successfully probed.

`GET /dashboard-config.json` response:
```json
{
  "hosts": [
    { "name": "local", "url": "http://localhost:7878" },
    { "name": "spark", "url": "http://spark.local:7878" }
  ],
  "poll_interval_ms": 30000
}
```
- `poll_interval_ms`: `config.polling.health_interval_secs * 1000`.
- Host URLs: `http://{address}:{api_port}`.

### 4. `crates/scmux-daemon/src/db.rs`

- Add `last_seen TEXT` column to `hosts` table in `init_db` migration (if not present):
  ```sql
  ALTER TABLE hosts ADD COLUMN last_seen TEXT;
  ```
- Add helper: `pub fn update_host_last_seen(conn: &Connection, host_id: i64, last_seen: &str) -> anyhow::Result<()>`

### 5. Tests (inline `#[cfg(test)]` acceptable per QA-019 waiver)

- **T-I-10**: Call `poll_hosts` with a host whose address is `"192.0.2.1"` (TEST-NET, always unreachable). Verify returns `Ok(())` — no panic.
- **T-I-11**: After T-I-10, verify `state.reachability` entry has `reachable == false`.
- **T-I-12**: Call `poll_hosts` with `is_local = true`. Verify entry has `reachable == true` and `last_seen` is `Some`.

Note: Tests must not require network access. Use `is_local = true` for the positive case (T-I-12) and an RFC 5737 TEST-NET address for the negative case (T-I-10/11).

## Acceptance Criteria

- `GET /hosts` includes `reachable` and `last_seen` per host.
- Local hosts always `reachable: true`.
- Remote host probe via `ping`; unreachable hosts do not crash daemon.
- `last_seen` set on successful probe, preserved on failure.
- T-I-10..T-I-12 pass without network access.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test --workspace` passes.

## Requirement IDs Covered

- `MH-01..MH-09`
- `API-14`, `API-15`
- `T-I-10`, `T-I-11`, `T-I-12`

## Dependencies

- Requires Sprint `1.2` merged into `integrate/phase-1`, then `integrate/phase-1` merged into `integrate/phase-2`.
- Must merge before Sprint `2.2`.
