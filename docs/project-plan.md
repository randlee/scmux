# scmux — Project Plan

## Overview

scmux is built in 4 phases. Each phase ends with a PR from `develop` → `main` and a version tag.

| Phase | Theme | Target Version |
|-------|-------|----------------|
| 1 | Daemon Core | 0.1.0 |
| 2 | Terminal + Dashboard | 0.2.0 |
| 3 | CI Integration + CLI | 0.3.0 |
| 4 | Hardening + Release | 1.0.0 |

**Branch strategy:**
- Sprint branches → `integrate/phase-N` → `develop` → `main` (at phase completion)
- All PRs target `integrate/phase-N` except final phase merge
- Worktrees at `../scmux-worktrees/<branch-name>` via `sc-git-worktree`

---

## Phase 1 — Daemon Core

**Goal:** Daemon compiles, runs, persists sessions, and exposes the full API surface.

### Sprint S0 — Foundation

**Acceptance:** `cargo build --workspace` clean; unit tests T-D-01..T-D-07 pass.

| ID | Task | Req IDs |
|----|------|---------|
| S0-1 | Config struct + load `~/.config/scmux/scmux.toml` | DG-04 |
| S0-2 | Add `config: Config` to `AppState` | arch §4.1 |
| S0-3 | Seed remote hosts from config into `hosts` table on first run | DG-05 |
| S0-4 | Set default log level to INFO | DG-07 |
| S0-5 | Fix DB schema parity: add `sessions_updated_at` trigger; fix index names | F-22, F-23 |
| S0-6 | Unit tests: `db::open()` (T-D-01..T-D-02), `ensure_local_host` (T-D-03..T-D-04), `should_run_now` (T-D-05..T-D-07) | T-D-01..07 |

### Sprint S1 — Full API Surface

**Acceptance:** All API endpoints respond; integration tests T-I-01..T-I-09 pass; API tests T-A-01..T-A-14 pass.

| ID | Task | Req IDs |
|----|------|---------|
| S1-1 | `POST /sessions` — register session with config validation | API-11, SR-02 |
| S1-2 | `PATCH /sessions/:name` — update cron, auto_start, config | API-12 |
| S1-3 | `DELETE /sessions/:name` — soft-delete via `enabled=0` | API-13, SR-04 |
| S1-4 | `GET /hosts` — list hosts with reachability flag | API-14 |
| S1-5 | `GET /dashboard-config.json` — host list + settings | API-15 |
| S1-6 | `POST /sessions/:name/start` + `stop` — with event logging | API-08, API-09, API-16 |
| S1-7 | `GET /` — serve dashboard static files | DG-03 |
| S1-8 | `POST /sessions/:name/jump` — iTerm2 AppleScript (local + SSH) | TL-01..TL-06 |
| S1-9 | Integration tests T-I-01..T-I-09 | T-I-01..09 |
| S1-10 | API tests T-A-01..T-A-14 | T-A-01..14 |

**Phase 1 completion gate:**
- `cargo build --release --workspace` clean, zero warnings
- All T-D, T-I, T-A tests pass
- Daemon starts, creates DB, serves `/health`
- Session registered via API starts via `auto_start` within 30s

---

## Phase 2 — Terminal + Dashboard

**Goal:** Jump works end-to-end; multi-host reachability; dashboard shows live data.

### Sprint S2 — Jump + Multi-Host + Live Dashboard

| ID | Task | Req IDs |
|----|------|---------|
| S2-1 | iTerm2 AppleScript polish + terminal override from request body | TL-07, TL-08 |
| S2-2 | Remote host health polling — `last_seen`, reachability flag | MH-03..MH-08 |
| S2-3 | `/hosts` response includes `reachable` + `last_seen` | MH-05 |
| S2-4 | Dashboard: swap mock data for live API fetch | DV-01..DV-11 |
| S2-5 | Dashboard: monochrome rendering for unreachable hosts | DV-06, MH-06 |
| S2-6 | Dashboard: "last seen N ago" indicator | MH-07 |
| S2-7 | Dashboard: jump modal — pane list, CI badges, "Open in iTerm2" button | DJ-01..DJ-07 |
| S2-8 | Dashboard: Grid / List / Grouped views + filters | DV-01..DV-10 |
| S2-9 | Dashboard: header counts | DV-11 |
| S2-10 | Manual UI tests T-UI-01..T-UI-17 | T-UI-01..17 |

**Phase 2 completion gate:**
- Jump via dashboard opens iTerm2 attached to correct session
- Disconnecting VPN → host goes monochrome, no error dialog
- Reconnecting VPN → host resumes full color within one poll cycle

---

## Phase 3 — CI Integration + CLI

**Goal:** CI badges on dashboard; full `scmux` CLI operational.

### Sprint S3a — CI Integration

| ID | Task | Req IDs |
|----|------|---------|
| S3a-1 | Detect `gh` and `az` in PATH at startup | CI-03 |
| S3a-2 | Record `tool_unavailable` in `session_ci` when CLI missing | CI-04 |
| S3a-3 | `ci_loop`: adaptive 1min (active) / 5min (idle) intervals | CI-05..CI-08 |
| S3a-4 | GitHub: `gh pr list` + `gh run list` → `session_ci` | CI-09, CI-12 |
| S3a-5 | Azure: `az pipelines` → `session_ci` | CI-10, CI-13 |
| S3a-6 | Dashboard: CI badges + tool-unavailable tooltip | DC-01..DC-05 |
| S3a-7 | Tests: T-D-10..T-D-13, T-A-09..T-A-10 | T-D-10..13 |

### Sprint S3b — CLI Binary

| ID | Task | Req IDs |
|----|------|---------|
| S3b-1 | Add `reqwest`, `clap`, `tokio` to `crates/scmux/Cargo.toml` | CLI-01..03 |
| S3b-2 | `scmux list` / `show` | CLI-04..05 |
| S3b-3 | `scmux start` / `stop` / `jump` | CLI-06..08 |
| S3b-4 | `scmux add` / `edit` / `disable` / `enable` / `remove` | CLI-09..12 |
| S3b-5 | `scmux hosts` / `scmux daemon status` | CLI-13..14 |
| S3b-6 | `TMS_HOST` → `SCMUX_HOST` env var support + `--host` flag | CLI-03 |

**Phase 3 completion gate:**
- Missing `gh`/`az` shows grayed badges with install tooltip
- `scmux list` matches dashboard data
- `scmux jump <name>` opens iTerm2 via daemon

---

## Phase 4 — Hardening + Release

**Goal:** Production-ready, fully tested, published.

### Sprint S4 — Hardening + Acceptance

| ID | Task | Req IDs |
|----|------|---------|
| S4-1 | launchd plist: `com.scmux.scmux-daemon.plist` | DH-04 |
| S4-2 | systemd unit: `scmux-daemon.service` | DH-05 |
| S4-3 | `scmux daemon status` command | CLI-14 |
| S4-4 | Performance pass: poll cycle <500ms / 50 sessions | NF-03 |
| S4-5 | HTTP read endpoints <100ms | NF-04 |
| S4-6 | Resilience: no crash on single-host/session failure | NF-08 |
| S4-7 | E2E tests T-E-01..T-E-11 | T-E-01..11 |
| S4-8 | `cargo build --release --workspace` — zero warnings | AC-1 |
| S4-9 | 24-hour daemon stability run on macOS | AC-2 |
| S4-10 | Homebrew formula in `randlee/homebrew-tap` | release |
| S4-11 | Publish `scmux-daemon` + `scmux` to crates.io | release |

**Phase 4 completion gate (Acceptance Criteria from requirements §7):**
1. `cargo build --release --workspace` clean, zero warnings
2. Daemon survives 24h on macOS without crashing
3. All T-D, T-I, T-A tests pass
4. Dashboard shows real live data
5. Jump via dashboard opens iTerm2 correctly
6. `auto_start` session killed externally restarts within 30s
7. Cron-scheduled session starts within 15s of scheduled time
8. VPN disconnect → no error dialogs; host shows monochrome
9. VPN reconnect → full color within one poll cycle
10. Missing `gh`/`az` → grayed badges with install tooltip

---

## Agent Execution Model

| Role | Agent | Responsibility |
|------|-------|----------------|
| team-lead | Claude Code (you) | Coordination, task assignment, reviews, merges |
| arch-cmux | Codex (`scmux-dev` tmux pane) | Implementation work |
| publisher | Claude Code agent | Release gates and publishing |
| scmux-qa | Background agent | Compliance validation before phase merges |

---

## Current Status

| Sprint | Status |
|--------|--------|
| S0 | 🔄 In progress (arch-cmux) |
| S1–S4 | Pending |
