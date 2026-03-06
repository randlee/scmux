# Sprint 2.1 — Multi-Host Reachability

- Sprint ID: `2.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s1-multihost`
- Base branch: `integrate/phase-2`
- PR target: `integrate/phase-2`

## Context

Host definitions are seeded, but no active reachability loop or stale-data handling contract is implemented.

## Deliverables

1. `crates/scmux-daemon/src/hosts.rs` (new)
- Add:
  - `pub struct HostReachability { pub host_id: i64, pub reachable: bool, pub last_seen: Option<String> }`
  - `pub async fn poll_hosts(state: Arc<AppState>) -> anyhow::Result<()>`
- Implement per-host health probing and in-memory reachability map updates.

2. `crates/scmux-daemon/src/main.rs`
- Spawn `host_poll_loop` with configured interval.
- Ensure loop failures are logged and non-fatal.

3. `crates/scmux-daemon/src/api.rs`
- `/hosts` includes `reachable` and `last_seen` fields.
- `/dashboard-config.json` includes host URLs and dashboard poll interval.

4. `crates/scmux-daemon/src/db.rs`
- Persist `last_seen` updates for successful host probes.

5. `tests/integration_tests.rs`
- Add/complete:
  - `T-I-10` unreachable remote host does not crash loop
  - `T-I-11` host marked unreachable on timeout
  - `T-I-12` host returns to reachable state

## Acceptance Criteria

- Reachability is tracked per host and exposed in `/hosts`.
- Unreachable hosts do not trigger daemon crashes or hard errors.
- `last_seen` updates only on successful probe.
- T-I-10..T-I-12 pass.

## Requirement IDs Covered

- `MH-01..MH-09`
- `API-14`, `API-15`
- `T-I-10`, `T-I-11`, `T-I-12`

## Dependencies

- Requires Sprint `1.2` merged.
- Must merge before Sprint `2.2`.
