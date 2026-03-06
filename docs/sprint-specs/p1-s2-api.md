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

5. `tests/integration_tests.rs`
- Implement `T-I-01..T-I-07` lifecycle cases.

## Acceptance Criteria

- API supports documented CRUD + jump + host/dashboard endpoints.
- `POST /sessions/:name/jump` returns `{ ok, message }` success/failure responses.
- Session add/edit/remove operations work and persist as expected.
- Start/stop/jump all produce event rows in `session_events`.
- T-A-01..T-A-11 and T-I-01..T-I-07 pass.

## Requirement IDs Covered

- `DG-03`
- `TL-01..TL-08`
- `API-08..API-16`
- `SR-02`, `SR-04`
- `T-A-01..T-A-11`
- `T-I-01..T-I-07`

## Dependencies

- Requires Sprint `1.1` merged.
- Must merge before Sprint `2.1`.
