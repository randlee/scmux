# Phase 6 Critical Review Fix Pass

## Summary

Fix pass addressing all blocking findings from the three-reviewer critical code review
(scmux-qa, rust-qa, arch-cmux) conducted prior to v0.5.0 release.

Worktree: `feature/p6-critical-review-fixes` (off `integrate/phase-6`)

## Design Decisions (Owner-Approved)

| Decision | Direction |
|----------|-----------|
| ATM auto-discovery | **Removed entirely.** `discover_teams()` filesystem scan of `~/.claude/teams/` is out-of-policy. ATM team list must come from explicit config (`atm.teams`). |
| ATM defaults | `atm.enabled = false`, `atm.allow_shutdown = false` — opt-in only |
| `daemon_health` SQLite | **Removed.** No runtime telemetry to SQLite without explicit design approval. Health tracking moves to AppState in-memory. |
| `scmux doctor` | New CLI subcommand added to expose daemon/runtime health signals (replaces daemon_health table visibility) |
| CLI write commands | **PENDING** owner decision — `add/edit/remove` subcommands not touched in this pass |

## Blocking Findings (all must be resolved)

| ID | File:Line | Category | Fix |
|----|-----------|----------|-----|
| B-01 | `api.rs:524` | ERROR HANDLING | `POST /start` returns 4xx/5xx on failure, not HTTP 200 with `ok:false` |
| B-02 | `api.rs:259` | ERROR HANDLING | `list_sessions` returns HTTP 500 on DB/JoinError, not empty 200 |
| B-03 | `db.rs:382` | ERROR HANDLING | `update_host` propagates DB error with `?` instead of `.unwrap_or(false)` |
| B-04 | `integration_tests.rs:147,467` | TEST NAMING | Rename two tests whose names contradict their assertions |
| B-05 | `db.rs:138,193` | ERROR HANDLING | Replace `filter_map(Result::ok)` with `collect::<Result<Vec<_>,_>>()?` |
| B-06 | `db.rs:330` | CODE QUALITY | Replace `let _ = write!(...)` with `sql.push_str(...)` |
| B-07 | `api.rs:241,248` | ERROR HANDLING | `health` handler: `.unwrap()` → `.expect("db lock")`; log JoinError instead of `.unwrap_or(0)` |
| B-08 | `api.rs:553` | ERROR HANDLING | `let _ = atm::send_shutdown_messages(...)` → `if let Err(e) = ... { tracing::warn! }` |
| B-09 | `atm.rs:220,145` | ARCHITECTURE | Remove `discover_teams()` filesystem scan; gate all socket calls on `atm.enabled`; add team allowlist config |
| B-10 | `db.rs:440,506` | ARCHITECTURE | Remove `daemon_health` table from `migrate()` and `write_health()` DB writes; move to in-memory AppState |
| NEW | `cli/main.rs` | FEATURE | Add `scmux doctor` subcommand — queries `GET /health`, prints runtime signals |

## Acceptance Criteria

- `cargo clippy --workspace --all-targets -- -D warnings` PASS
- `cargo test --workspace` PASS
- No `daemon_health` table created after `migrate()`
- `atm::poll_once` returns immediately when `atm.enabled = false`
- `atm::send_shutdown_messages` is no-op when `atm.allow_shutdown = false`
- `POST /sessions/:name/start` returns non-200 HTTP status on start failure
- `scmux doctor` compiles and calls `GET /health`

## Out of Scope (this pass)

- B-11: CLI write commands (`add/edit/remove`) — pending owner decision
- GH #31, #35, #36, #37 — deferred non-blocking findings
- Non-blocking important findings (I-01 through I-06)
