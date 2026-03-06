# Sprint 3.1 — CI Integration

- Sprint ID: `3.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p3-s1-ci`
- Base branch: `integrate/phase-3`
- PR target: `integrate/phase-3`

## Context

`session_ci` table exists but provider polling and adaptive scheduling are not yet implemented.

## Deliverables

1. `crates/scmux-daemon/src/ci.rs` (new)
- Add provider pollers:
  - `poll_github(session: &Session) -> ProviderResult`
  - `poll_azure(session: &Session) -> ProviderResult`
- Add availability detection:
  - `detect_tools() -> ToolAvailability`
- Add adaptive interval selection:
  - `next_interval(has_active_pane: bool) -> Duration`

2. `crates/scmux-daemon/src/main.rs`
- Add `ci_loop` task spawn.
- Use 1-minute active / 5-minute idle cadence per session.

3. `crates/scmux-daemon/src/db.rs`
- Add upsert helpers for `session_ci` rows including:
  - `status = tool_unavailable`
  - `tool_message`
  - `polled_at`
  - `next_poll_at`

4. `crates/scmux-daemon/src/api.rs`
- Extend session responses with CI summary payload needed by dashboard.

5. Tests
- `tests/ci_tests.rs` (new): `T-D-10..T-D-13`.
- API-level checks for tool unavailable payload persistence.

## Acceptance Criteria

- `gh`/`az` detection runs at startup and influences CI poll behavior.
- Missing provider tools produce persisted `tool_unavailable` rows in `session_ci`.
- Active/idle polling cadence follows requirements.
- CI payload is available via API responses.
- T-D-17: Network failure during CI fetch is caught; session_ci row updated with error status; daemon continues running.
- T-D-18: Auth or rate-limit error during CI fetch is caught; session_ci row updated with error status; daemon continues running.

## Requirement IDs Covered

- `CI-01..CI-13`
- `DC-01..DC-05`
- `NF-07`
- `T-D-10`, `T-D-11`, `T-D-12`, `T-D-13`, `T-D-17`, `T-D-18`

## Dependencies

- Requires Sprint `2.2` merged.
- Must merge before Sprint `3.2`.
