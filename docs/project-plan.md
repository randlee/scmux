# scmux — Project Plan

## Overview

scmux is delivered in four phases with explicit integration branches and version targets.

| Phase | Theme | Target Version | Integration Branch |
|-------|-------|----------------|--------------------|
| 1 | Foundation + API | 0.1.0 | `integrate/phase-1` |
| 2 | Multi-host + Dashboard | 0.2.0 | `integrate/phase-2` |
| 3 | CI + CLI | 0.3.0 | `integrate/phase-3` |
| 4 | Supervision + Release | 1.0.0 | `integrate/phase-4` |

## Execution Model

### Roles and ownership

- `team-lead` owns sequencing, review, and merge decisions.
- `arch-cmux` is the sole implementation agent for sprint delivery work.
- `quality-mgr` runs dual QA tracks (`rust-qa-agent` + `scmux-qa-agent`) in parallel.

### Branching and worktrees

- Main repo remains on `develop`.
- Each sprint runs in a dedicated worktree at `/Users/randlee/Documents/github/scmux-worktrees/<branch>`.
- Sprint PRs target the phase integration branch.
- Phase integration branches merge to `develop` after phase completion.

### ATM communication protocol

- Assignment: `team-lead` sends direct ATM message to `arch-cmux`.
- Ack: `arch-cmux` acknowledges receipt before coding.
- Completion: `arch-cmux` reports commit/PR back via ATM.
- Follow-up: QA findings are sent by `quality-mgr` to `team-lead`, then forwarded as fix tasks.

## Sprint Status

| Sprint | Status |
|--------|--------|
| S0 | Complete (foundation: cargo build, workspace scaffold, env var rename) |
| S1.1 | Pending |
| S1.2 | Pending |
| S2.1 | Pending |
| S2.2 | Pending |
| S3.1 | Pending |
| S3.2 | Pending |
| S4.1 | Pending |
| S4.2 | Pending |

## Sprint S0 — Foundation (Complete)

**Status:** Complete as of initial repo setup (before formal sprint tracking began).

**Delivered:**
- `cargo build --workspace` clean with no errors
- Workspace scaffold: `crates/scmux-daemon`, `crates/scmux`
- `tms` → `scmux` rename throughout codebase
- `SCMUX_DB` and `SCMUX_PORT` env vars established

**Note:** Logging module (DG-08) was identified post-S0 and is assigned to Sprint S1.1.

## Phase 1 — Foundation + API

**Goal:** daemon config/database foundations are compliant and the HTTP surface is complete.
**Version:** `0.1.0`
**Integration branch:** `integrate/phase-1`

### Sprint 1.1 — Foundation (Config + DB)

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p1-s1-foundation`
- Base branch: `integrate/phase-1`
- PR target: `integrate/phase-1`
- Context: build is clean, but config loading/host seeding/schema parity gaps remain.
- Deliverables:
  - `crates/scmux-daemon/src/config.rs` with `Config` model matching architecture section 6 (`daemon`, `polling`, `hosts`).
  - `Config::load()` from `~/.config/scmux/scmux.toml` with defaults fallback.
  - `crates/scmux-daemon/src/main.rs` uses config for `port`, `db_path`, intervals; `AppState` adds `config: Config`; default INFO logging.
  - `crates/scmux-daemon/src/db.rs` adds `seed_hosts_from_config()`, `sessions_updated_at` trigger, schema index names matching `docs/schema.sql`.
  - `scmux.toml.example` at repo root.
  - `tests/db_tests.rs` for T-D-01..T-D-04.
  - `tests/scheduler_tests.rs` for T-D-05..T-D-07 (`should_run_now` exposed as `pub(crate)`).
- Acceptance criteria:
  - daemon starts with defaults when config file is missing.
  - config file values override defaults.
  - host seeding is idempotent and inserts configured remote hosts.
  - DB migration parity matches `docs/schema.sql` for trigger/index names.
  - T-D-01..T-D-07 pass.
- Requirement IDs: `DG-04`, `DG-05`, `DG-07`, `T-D-01..T-D-07`.
- Detailed spec: [docs/sprint-specs/p1-s1-foundation.md](./sprint-specs/p1-s1-foundation.md)

### Sprint 1.2 — Full API Surface

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p1-s2-api`
- Base branch: `feature/p1-s1-foundation`
- PR target: `integrate/phase-1`
- Context: foundational daemon loop exists; API endpoints and jump/static serving are incomplete.
- Deliverables:
  - `crates/scmux-daemon/src/api.rs` adds `POST /sessions`, `PATCH /sessions/:name`, `DELETE /sessions/:name`, `GET /hosts`, `GET /dashboard-config.json`, `POST /sessions/:name/jump`, `GET /`.
  - `crates/scmux-daemon/src/tmux.rs` / jump helper module implements iTerm2 AppleScript local+SSH launch.
  - `crates/scmux-daemon/src/db.rs` and handler validation enforce session registration constraints (`SR-02`, soft delete behavior).
  - event logging coverage for start/stop/jump actions (`API-16`).
  - `tests/api_tests.rs` covering T-A-01..T-A-11.
  - `tests/integration_tests.rs` covering T-I-01..T-I-07.
- Acceptance criteria:
  - full API routes respond with documented behavior/status.
  - jump endpoint returns structured `{ok,message}` for success/failure.
  - session CRUD flow works end-to-end with soft delete.
  - integration/API tests listed above pass.
- Requirement IDs: `API-08..API-16`, `TL-01..TL-08`, `DG-03`, `SR-02`, `SR-04`.
- Detailed spec: [docs/sprint-specs/p1-s2-api.md](./sprint-specs/p1-s2-api.md)

## Phase 2 — Multi-host + Dashboard

**Goal:** multi-host reachability is first-class and dashboard is live against daemon APIs.
**Version:** `0.2.0`
**Integration branch:** `integrate/phase-2`

### Sprint 2.1 — Multi-Host Reachability

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s1-multihost`
- Base branch: `integrate/phase-2`
- PR target: `integrate/phase-2`
- Context: hosts table exists, but active reachability model/last-seen behavior must be implemented.
- Deliverables:
  - `crates/scmux-daemon/src/hosts.rs` for host polling and in-memory reachability state.
  - `crates/scmux-daemon/src/main.rs` adds host poll loop scheduling.
  - `crates/scmux-daemon/src/api.rs` `/hosts` and dashboard config include `reachable` and `last_seen` fields.
  - integration tests for unreachable/restore behavior.
- Acceptance criteria:
  - unreachable hosts are represented without daemon errors.
  - last-known data remains available during outage.
  - reachability auto-recovers within one poll cycle.
- Requirement IDs: `MH-01..MH-09`, `T-I-10..T-I-12`.
- Detailed spec: [docs/sprint-specs/p2-s1-multihost.md](./sprint-specs/p2-s1-multihost.md)

### Sprint 2.2 — Live Dashboard

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s2-dashboard`
- Base branch: `feature/p2-s1-multihost`
- PR target: `integrate/phase-2`
- Context: dashboard has static mock data and needs live integration + host-aware rendering.
- Deliverables:
  - `dashboard/team-dashboard.jsx` switched to live fetch across hosts.
  - monochrome rendering for unreachable-host sessions.
  - jump modal wired to daemon `POST /sessions/:name/jump` and feedback handling.
  - grid/list/grouped + filters + header aggregate counts validated against API.
  - dashboard manual test checklist coverage.
- Acceptance criteria:
  - dashboard renders live daemon data from all configured hosts.
  - unreachable hosts are monochrome with `last seen` indicator.
  - jump modal executes daemon jump and renders response.
- Requirement IDs: `DV-01..DV-11`, `DC-01..DC-05`, `DJ-01..DJ-07`, `T-UI-01..T-UI-17`.
- Detailed spec: [docs/sprint-specs/p2-s2-dashboard.md](./sprint-specs/p2-s2-dashboard.md)

## Phase 3 — CI + CLI

**Goal:** CI signals are integrated and CLI is production-usable.
**Version:** `0.3.0`
**Integration branch:** `integrate/phase-3`

### Sprint 3.1 — CI Integration

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p3-s1-ci`
- Base branch: `integrate/phase-3`
- PR target: `integrate/phase-3`
- Context: schema has `session_ci`; provider polling/tool-degradation behavior is not implemented.
- Deliverables:
  - `crates/scmux-daemon/src/ci.rs` for provider polling, parsing, persistence.
  - `crates/scmux-daemon/src/main.rs` adds `ci_loop` with adaptive interval.
  - tool discovery at startup for `gh`/`az`; `tool_unavailable` persisted to `session_ci`.
  - API summaries expose provider status payloads.
  - tests covering interval and tool-unavailable behavior.
- Acceptance criteria:
  - active sessions poll every 1 minute; idle/stopped every 5 minutes.
  - unavailable provider tools yield persisted `tool_unavailable` status.
  - provider payloads surface in API for dashboard display.
- Requirement IDs: `CI-01..CI-13`, `T-D-10..T-D-13`.
- Detailed spec: [docs/sprint-specs/p3-s1-ci.md](./sprint-specs/p3-s1-ci.md)

### Sprint 3.2 — CLI Binary

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p3-s2-cli`
- Base branch: `feature/p3-s1-ci`
- PR target: `integrate/phase-3`
- Context: CLI crate is only a stub and lacks network/command plumbing.
- Deliverables:
  - `crates/scmux/Cargo.toml` includes `reqwest`, `clap`, `tokio`.
  - `crates/scmux/src/main.rs` implements command tree and dispatch.
  - `crates/scmux/src/client.rs` HTTP client with `SCMUX_HOST`/`--host` support.
  - command coverage for list/show/start/stop/jump/add/edit/disable/enable/remove/hosts/daemon status.
- Acceptance criteria:
  - all CLI commands map to daemon API routes and return actionable output.
  - `scmux list` matches dashboard-visible state.
  - `scmux jump` triggers daemon jump route successfully.
- Requirement IDs: `CLI-01..CLI-14`.
- Detailed spec: [docs/sprint-specs/p3-s2-cli.md](./sprint-specs/p3-s2-cli.md)

## Phase 4 — Supervision + Release

**Goal:** production hardening, end-to-end validation, and 1.0 release readiness.
**Version:** `1.0.0`
**Integration branch:** `integrate/phase-4`

### Sprint 4.1 — Supervision + Performance

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s1-supervision`
- Base branch: `integrate/phase-4`
- PR target: `integrate/phase-4`
- Context: core features exist; service lifecycle and perf guarantees are not finalized.
- Deliverables:
  - launchd + systemd service assets and install/run docs.
  - daemon status command and health telemetry refinements.
  - profiling/optimization pass for poll/API latency constraints.
- Acceptance criteria:
  - boot supervision works on macOS and Linux.
  - NF-03/NF-04 performance targets are measured and met.
  - daemon remains resilient under partial-host/session failure.
- Requirement IDs: `DH-04`, `DH-05`, `NF-01..NF-05`, `NF-08`.
- Detailed spec: [docs/sprint-specs/p4-s1-supervision.md](./sprint-specs/p4-s1-supervision.md)

### Sprint 4.2 — E2E Tests + Release

- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s2-release`
- Base branch: `feature/p4-s1-supervision`
- PR target: `integrate/phase-4`
- Context: all feature work merged; final verification and release packaging required.
- Deliverables:
  - complete end-to-end suite T-E-01..T-E-11.
  - acceptance criteria validation report.
  - release artifacts/versioning to `1.0.0` and Homebrew publish steps.
- Acceptance criteria:
  - all E2E tests pass consistently.
  - section 7 acceptance criteria in requirements are fully satisfied.
  - release checklist complete for binary/crate/Homebrew channels.
- Requirement IDs: `T-E-01..T-E-11`, `AC-1..AC-10`.
- Detailed spec: [docs/sprint-specs/p4-s2-release.md](./sprint-specs/p4-s2-release.md)

## Dependencies Across Sprints

- `1.1` must merge before `1.2`.
- `1.2` must merge before any phase 2 sprint.
- `2.1` must merge before `2.2`.
- `2.2` should merge before phase 3 UI-facing CI badge validation.
- `3.1` must merge before `3.2`.
- `3.2` must merge before any phase 4 sprint.
- `4.1` must merge before `4.2`.

## Current Phase Entry Point

- Active planning baseline: `Phase 1`.
- Next implementation sprint: `1.1`.
- First QA gate: after `1.1` PR opens on `integrate/phase-1`.
