# Sprint 1.2 — Full API Surface

- Sprint ID: `1.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p1-s2-api`
- Base branch: `feature/p1-s1-foundation`
- PR target: `integrate/phase-1`

## Context

Core daemon routes exist for health/list/detail/start/stop only. Missing endpoints block dashboard integration and session lifecycle management completeness.

## Deliverables

1. `crates/scmux-daemon/src/api.rs`
- Add routes/handlers:
  - `GET /` static dashboard index (DG-03)
  - `POST /sessions` (API-11)
  - `PATCH /sessions/:name` (API-12)
  - `DELETE /sessions/:name` soft-delete (API-13)
  - `GET /hosts` (API-14)
  - `GET /dashboard-config.json` (API-15)
  - `POST /sessions/:name/jump` (TL-01, API-10)
- Add request models:
  - `CreateSessionRequest`
  - `PatchSessionRequest`
  - `JumpRequest { terminal: Option<String> }`
- Add response models:
  - `HostSummary`
  - `DashboardConfigResponse`
- Ensure action routes log to `session_events` (API-16).

2. `crates/scmux-daemon/src/tmux.rs` (or new `jump.rs`)
- Add function:
  - `pub async fn jump_session(host: HostTarget, session: &str, terminal: &str) -> anyhow::Result<String>`
- Implement iTerm2 AppleScript launch for:
  - local command `tmux attach -t <session>`
  - remote command `ssh <user>@<host> tmux attach -t <session>`

3. `crates/scmux-daemon/src/db.rs`
- Add session create/update/soft-delete helpers used by API handlers.
- Validate `config_json.session_name` equals session `name` (`SR-02`).

4. `tests/api_tests.rs`
- Implement coverage for `T-A-01..T-A-11` at minimum, plus route-not-found checks.
- Add coverage for:
  - `T-A-12`: `PATCH /sessions/:name` updates `cron_schedule`
  - `T-A-13`: `DELETE /sessions/:name` sets `enabled=0`
  - `T-A-14`: `GET /hosts` returns all hosts with reachability flag

5. `tests/integration_tests.rs`
- Implement `T-I-01..T-I-07` lifecycle cases.
- Add:
  - `T-I-08`: `write_health()` inserts a `daemon_health` row
  - `T-I-09`: `write_health()` prunes rows older than 7 days

### Deliverable: Session lifecycle enforcement (scheduler.rs)

Ensure `poll_cycle()` in `scheduler.rs` enforces `SL-01..SL-11`:
- `SL-01..SL-03`: detect and transition running/stopped states
- `SL-04..SL-06`: `auto_start` restart behavior
- `SL-07..SL-09`: cron scheduling logic
- `SL-10..SL-11`: event logging on state transitions

Ensure `tmux.rs` implements `PS-01..PS-06`:
- `PS-01..PS-03`: list panes for a session and their status
- `PS-04..PS-06`: store pane data in `sessions_panes` table

## Acceptance Criteria

- API supports documented CRUD + jump + host/dashboard endpoints.
- `POST /sessions/:name/jump` returns `{ ok, message }` success/failure responses.
- Session add/edit/remove operations work and persist as expected.
- Start/stop/jump all produce event rows in `session_events`.
- T-A-01..T-A-11 and T-I-01..T-I-07 pass.
- `T-A-12`: `PATCH /sessions/:name` updates `cron_schedule` and persists correctly.
- `T-A-13`: `DELETE /sessions/:name` sets `enabled=0` (soft delete).
- `T-A-14`: `GET /hosts` returns all hosts with `reachable` flag.
- `T-I-08`: `write_health()` inserts a `daemon_health` row on each call.
- `T-I-09`: `write_health()` prunes `daemon_health` rows older than 7 days.
- `T-D-08`: `tmux::live_sessions()` returns all active tmux sessions on the local host.
- `T-D-09`: `tmux::live_sessions()` returns an empty vec (not an error) when tmux is not running.
- Poll cycle correctly detects a killed session and marks it stopped within one cycle.
- Poll cycle restarts an `auto_start` session killed externally within 30s.
- Pane data is stored as JSON in `session_status.panes_json` on each poll.

## Requirement IDs Covered

- `DG-02`, `DG-03`, `DG-06`
- `TL-01..TL-08`
- `API-01..API-16`
- `SL-01..SL-11`
- `PS-01..PS-06`
- `SR-02`, `SR-03`, `SR-04`, `SR-05`, `SR-06`
- `T-A-01..T-A-14`
- `T-D-08`, `T-D-09`
- `T-I-01..T-I-09`

## Dependencies

- Requires Sprint `1.1` merged.
- Must merge before Sprint `2.1`.
