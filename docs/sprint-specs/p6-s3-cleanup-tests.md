# Sprint 6.3 — Cleanup, Tests & Deferred Finding Resolution

## Summary

Sprint 6.3 resolves deferred QA findings from Sprints 6.1 and 6.2, focusing on:
- Removing the `recent_events` dead API field from daemon and CLI
- Completing lifecycle test coverage
- Closing tracked GH issues from earlier sprint QA passes

## Scope

### Primary Deliverable

**GH #34** — Remove `recent_events` from `SessionDetail` API response:
- `crates/scmux-daemon/src/api.rs`: drop `EventRow` struct and `recent_events` field from `SessionDetail`; remove `recent_events: Vec::new()` from `get_session()`
- `crates/scmux-daemon/tests/api_tests.rs`: update `t_a_04` to assert field absence; rename test to `t_a_04_get_sessions_name_returns_200_with_config`
- `crates/scmux/src/client.rs`: drop `EventRow` struct and `recent_events` field from CLI-side `SessionDetail`

### Carry-Forward (from S6.1)

- **GH #33** (SCMUX-QA-P6S1-008): T-LC tests — verify named lifecycle tests exist for T-LC-02 and T-LC-05

## Acceptance Criteria

| Req ID | Criterion |
|--------|-----------|
| API-01 | `GET /sessions/:name` response does NOT include `recent_events` field |
| API-01 | Test `t_a_04` asserts `body.get("recent_events").is_none()` |
| T-LC | `cargo test --workspace` includes named tests covering session lifecycle transitions |
| NF-01 | `cargo clippy --workspace --all-targets -- -D warnings` exits zero |
| NF-01 | `cargo test --workspace` all pass |

## Requirement Coverage

- **DG-02** (definition-only writes) — no regression from S6.1/S6.2 write-gate
- **SL-01..SL-06** (lifecycle) — T-LC tests cover state transitions
- **API-01** (session detail) — `recent_events` removed; response matches current schema

## Out of Scope (tracked as GH issues for future)

| GH Issue | Finding | Sprint Target |
|----------|---------|---------------|
| #31 | Per-pane ATM fields in `PaneInfo` API response (PS-03) | Future |
| #35 | CI numeric summary counts (CI-10) | Future |
| #36 | Expandable PR list with links (DC-02/DC-03) | Future |
| #37 | Dashboard JSX source file in repo (DC-01) | Future |

## Notes

This sprint was the final sprint of Phase 6. After merge, the integration branch
`integrate/phase-6` was promoted to `develop` via a single consolidating PR.
