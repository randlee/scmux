# scmux — Requirements

## 1. Problem Statement

When running 20–30 concurrent Claude Code agent teams across multiple machines and terminal emulators (iTerm2, WezTerm, Warp), there is no way to:
- Know which tmux sessions exist and whether agents are alive
- Find a specific team quickly without hunting through terminal windows
- Start sessions on a schedule without manual intervention
- View all teams at a glance from one place
- Jump directly to any session on any machine in one action
- Monitor CI/PR status per team without switching contexts

## 2. Goals

1. **Single source of truth** — daemon owns SQLite; CLI and web UI are clients only
2. **Zero manual hunting** — any session reachable in ≤ 2 clicks from dashboard
3. **Unattended operation** — sessions start automatically on schedule or machine boot
4. **Multi-machine** — Mac + DGX Spark, with graceful handling of VPN-gated hosts
5. **Graceful degradation** — missing tools (gh, az) and unreachable hosts are normal states, not errors
6. **Non-invasive** — does not modify tmux config, agent prompts, or Claude Code setup

## 3. Non-Goals (v1)

- Not a terminal emulator replacement
- Not a Claude Code agent orchestrator (ATM's responsibility)
- No authentication or access control (local network only)
- No browser-based terminal (deferred to v2)
- No agent output streaming or log viewing
- WezTerm and Warp jump support deferred to post-MVP

---

## 4. Functional Requirements

### 4.1 Daemon — General

| ID | Requirement | Sprint |
|----|-------------|--------|
| DG-01 | The daemon shall be a single self-contained binary (`scmux-daemon`) | 1.1 |
| DG-02 | SQLite shall be a definition store only (projects/hosts/approved roster edits), not a tmux discovery cache | 6.0 |
| DG-03 | Persistent SQLite writes shall be allowed only through a dedicated `definition_writer` module; persistent-write functions in `db.rs` shall be visibility-restricted (`pub(crate)`/`pub(super)`) so non-editor modules cannot call them directly | 6.0 |
| DG-04 | Approved-project policy: a project is approved when created via the editor write path and valid with `enabled = 1` and non-empty `config_json.panes[]`; `definition_writer` shall reject persistent writes for non-approved/invalid projects | 6.0 |
| DG-05 | Poller modules (`tmux_poller`, `hosts`, `ci`, `atm`) shall not persist runtime observations to SQLite; runtime state shall be served from an in-memory projection layer | 6.0 |
| DG-06 | Deleting SQLite definitions shall not trigger reconstruction from tmux discovery; missing definitions remain missing until user redefines them | 6.0 |
| DG-07 | The daemon shall serve the web dashboard as static files at `GET /` | 1.2 |
| DG-08 | The daemon shall load configuration from `~/.config/scmux/scmux.toml` at startup | 1.1 |
| DG-09 | The daemon shall apply SQLite schema migrations on every startup (idempotent) | 1.1 |
| DG-10 | Logging paths shall be structured and OpenTelemetry-ready (trace context propagation and consistent event attributes) for near-term OTel integration | 6.0 |
| DG-11 | On panic or partial failure, the daemon shall isolate failures and shall not mass-stop unrelated sessions/agents | 6.0 |
| DG-12 | Runtime state is live/ephemeral and shall not be treated as persistent source-of-truth data in SQLite | 6.0 |
| DG-13 | Legacy runtime cache tables (`session_status`, `session_ci`, `session_atm`) are deprecated in P6 and replaced by in-memory projection for API responses | 6.0 |

### 4.2 Daemon — Session Lifecycle

| ID | Requirement | Sprint |
|----|-------------|--------|
| SL-01 | Runtime state machine shall be: `stopped -> starting -> running -> idle -> done` | 6.0 |
| SL-02 | `POST /sessions/:name/start` shall load the project definition from SQLite `config_json`, create tmux layout, and launch all pane commands without requiring iTerm | 6.0 |
| SL-03 | `running` means tmux session exists and at least one configured agent is ATM-active | 6.0 |
| SL-04 | `idle` means tmux session exists and all configured agents are ATM-idle/offline | 6.0 |
| SL-05 | `done` semantics are provisional and shall be finalized in a dedicated lifecycle decision; auto-teardown shall default to disabled until finalized | 6.0 |
| SL-06 | `POST /sessions/:name/stop` shall be graceful-first: send ATM shutdown signal/message, wait a configurable grace period, then escalate to scoped hard-stop only if still running; escalation parameters are configurable and subject to product-level finalization | 6.0 |
| SL-07 | Start/stop failures shall be isolated to the target session and shall not cascade to other sessions | 6.0 |
| SL-08 | Polling tmux/ATM/CI shall update runtime view only and shall not persist discovery-derived project definitions | 6.0 |
| SL-09 | Auto-start/cron may trigger `start` only for already-defined projects in SQLite | 6.0 |
| SL-10 | If `starting` fails (tmux/session creation or pane launch error), the session shall transition back to `stopped` with structured error details | 6.0 |

### 4.3 Daemon — Pane Status

| ID | Requirement | Sprint |
|----|-------------|--------|
| PS-01 | Pane identity shall be definition-driven from `config_json.panes[]` (`name`, `command`, `atm_agent`, `atm_team`) | 6.0 |
| PS-02 | Per-pane runtime state shall be resolved from ATM first (`active`, `idle`, `stuck`, `offline`) and tmux signals second | 6.0 |
| PS-03 | Pane presentation shall include: pane name, configured command, ATM agent/team mapping, current runtime state, and optional current task | 6.0 |
| PS-04 | Missing ATM data shall degrade to `unknown` without failing session rendering | 6.0 |
| PS-05 | Pane runtime snapshots are derived data; pollers shall not persist them as project-definition writes | 6.0 |
| PS-06 | Per-pane runtime state shall be ephemeral in-memory projection data (not persisted runtime cache tables) | 6.0 |

### 4.4 Daemon — Terminal Launch (Jump)

| ID | Requirement | Sprint |
|----|-------------|--------|
| TL-01 | The daemon shall handle `POST /sessions/:name/jump` and spawn the terminal process | 1.2 |
| TL-02 | The browser shall never launch terminal processes directly | 1.2 |
| TL-03 | For MVP, iTerm2 shall be the supported terminal, launched via AppleScript | 1.2 |
| TL-04 | For a local session, the command shall be: `tmux attach -t <name>` | 1.2 |
| TL-05 | For a remote session, the command shall be: `ssh <user>@<host> tmux attach -t <name>` | 1.2 |
| TL-06 | The jump endpoint shall return `{ ok, message }` indicating success or failure | 1.2 |
| TL-07 | The default terminal shall be configurable in `scmux.toml` | 1.2 |
| TL-08 | A `terminal` field in the jump request body may override the default | 1.2 |

### 4.5 Daemon — CI Integration

| ID | Requirement | Sprint |
|----|-------------|--------|
| CI-01 | The daemon shall support two CI providers: GitHub (`gh` CLI) and Azure DevOps (`az` CLI) | 3.1 |
| CI-02 | Both providers are optional; sessions may have one, both, or neither | 3.1 |
| CI-03 | On startup, the daemon shall detect whether `gh` and `az` are available in PATH | 3.1 |
| CI-04 | If a required CLI tool is not available, the daemon shall expose `tool_unavailable` runtime status with install guidance | 3.1 |
| CI-05 | The daemon shall poll CI status on an adaptive interval per session | 3.1 |
| CI-06 | When any pane in a session has status `active`, the CI poll interval shall be 1 minute | 3.1 |
| CI-07 | When all panes are idle or session is stopped, the CI poll interval shall be 5 minutes | 3.1 |
| CI-08 | GitHub polling shall collect open PR list and recent workflow runs (`#`, title, URL, status, branch, timestamp) | 3.1 |
| CI-09 | Azure polling shall collect open PRs and pipeline run status | 3.1 |
| CI-10 | Project cards shall expose CI summary counts: active PRs, passing jobs, failing jobs, running jobs | 6.0 |
| CI-11 | CI polling modules shall not persist runtime snapshots to SQLite | 6.0 |
| CI-12 | The `github_repo` column on `sessions` shall hold the repo in `owner/repo` format | 3.1 |
| CI-13 | The `azure_project` column on `sessions` shall hold the Azure DevOps project URL or identifier | 3.1 |

### 4.6 Daemon — Health

| ID | Requirement | Sprint |
|----|-------------|--------|
| DH-01 | `GET /health` shall expose daemon liveness, uptime, and poller health | 1.1 |
| DH-02 | Host polling shall expose reachability, last-seen timestamp, and stale-data status for display | 2.1 |
| DH-03 | Single host/session poll errors shall not crash the daemon and shall not degrade unrelated project state | 4.1 |
| DH-04 | The daemon shall start automatically on machine boot (launchd / systemd) | 4.1 |
| DH-05 | The daemon shall restart automatically if it crashes | 4.1 |

### 4.7 HTTP API

| ID | Requirement | Sprint |
|----|-------------|--------|
| API-01 | The daemon shall expose HTTP on a configurable port (default 7878) | 1.2 |
| API-02 | All responses shall be JSON | 1.2 |
| API-03 | CORS shall be permissive | 1.2 |
| API-04 | `GET /health` — daemon status, uptime seconds, enabled session count, DB path, version | 1.2 |
| API-05 | `GET /sessions` — all defined projects with live runtime status (running or stopped), sourced from in-memory runtime projection + persisted definitions | 6.0 |
| API-06 | `GET /sessions/:name` — full detail: definition, pane mappings, ATM state, CI snapshot, sourced from in-memory runtime projection + persisted definitions | 6.0 |
| API-07 | `GET /sessions/:name` — 404 if not found | 1.2 |
| API-08 | `POST /sessions/:name/start` — launch from stored project definition, return ok/error | 6.0 |
| API-09 | `POST /sessions/:name/stop` — graceful-first shutdown path, return ok/error | 6.0 |
| API-10 | `POST /sessions/:name/jump` — spawn terminal, return ok/error | 1.2 |
| API-11 | `POST /sessions` — create project definition (persistent write path via writer subsystem; e.g. New Project editor entry point) | 6.0 |
| API-12 | `PATCH /sessions/:name` — update project definition (persistent write path via writer subsystem; e.g. Project Editor/card Edit entry points) | 6.0 |
| API-13 | `DELETE /sessions/:name` — remove/disable project definition (editor-only persistent write path) | 6.0 |
| API-14 | `GET /hosts` — list all hosts with reachability status | 1.2 |
| API-15 | `GET /dashboard-config.json` — host list + dashboard settings for web UI | 1.2 |
| API-16 | `GET /discovery` shall expose raw tmux discovery (including non-defined sessions) without mutating SQLite definitions | 6.0 |
| API-17 | `POST /sessions/:name/start` shall reject missing/malformed `config_json` with a structured validation error payload | 6.0 |

### 4.8 Multi-Host / VPN Handling

| ID | Requirement | Sprint |
|----|-------------|--------|
| MH-01 | Hosts shall be user-defined in the project/host editor and persisted in SQLite | 6.0 |
| MH-02 | The dashboard shall poll all known hosts independently | 2.1 |
| MH-03 | A host that is unreachable (timeout, network error) shall NOT be treated as an error condition | 2.1 |
| MH-04 | When a host is unreachable, the daemon shall retain the last known session data | 2.1 |
| MH-05 | Unreachable hosts shall be flagged `reachable: false` in the `/hosts` response | 2.1 |
| MH-06 | The dashboard shall render sessions from unreachable hosts in monochrome (grayscale) | 2.1 |
| MH-07 | The dashboard shall display a "last seen N ago" indicator per host when unreachable | 2.1 |
| MH-08 | When a host becomes reachable again, the dashboard shall resume full-color rendering automatically | 2.1 |
| MH-09 | No error dialogs or alerts shall be shown for VPN-gated host unavailability | 2.1 |

### 4.9 Dashboard — Views and Filtering

| ID | Requirement | Sprint |
|----|-------------|--------|
| DV-01 | The dashboard shall support three views: Grid, List, Grouped-by-project | 2.2 |
| DV-02 | Primary dashboard view shall show all defined projects (running or stopped), one card per project | 6.0 |
| DV-03 | List: table — name, project, status, pane count, active count, open PRs, last activity | 2.2 |
| DV-04 | Grouped: sessions under project headers with per-project running count and PR count | 2.2 |
| DV-05 | Stopped sessions shall be visually de-emphasized (reduced opacity) | 2.2 |
| DV-06 | Unreachable-host sessions shall be rendered in monochrome | 2.2 |
| DV-07 | Filter by status: all / running / idle / stopped | 2.2 |
| DV-08 | Filter by project | 2.2 |
| DV-09 | Text search by session name | 2.2 |
| DV-10 | Filters shall be combinable | 2.2 |
| DV-11 | Header shall show global counts: running, idle, stopped, active agents, open PRs | 2.2 |
| DV-12 | A secondary tab/view shall show `GET /discovery` raw tmux sessions not linked to defined projects; this view is informational only | 6.0 |
| DV-13 | Each project card shall provide an Edit affordance that opens the project editor for definition updates | 6.0 |
| DV-14 | The dashboard shall provide a `New Project` flow that creates project definitions through the `definition_writer` subsystem | 6.0 |

### 4.10 Dashboard — CI Display

| ID | Requirement | Sprint |
|----|-------------|--------|
| DC-01 | Each project card shall show CI summary badges for configured providers | 2.2 |
| DC-02 | GitHub: show expandable PR list with links and per-run status (green/yellow/red/running) | 6.0 |
| DC-03 | Azure: show pipeline status list with pass/fail/running indicators | 6.0 |
| DC-04 | When a tool is unavailable, show a grayed badge with tooltip: "Install gh CLI: brew install gh" | 2.2 |
| DC-05 | When a session has no CI configured, show nothing (no empty badge) | 2.2 |

### 4.11 Dashboard — Jump

| ID | Requirement | Sprint |
|----|-------------|--------|
| DJ-01 | Clicking a session card shall open a jump modal | 2.2 |
| DJ-02 | The modal shall show: name, project, pane list with status, CI/PR details, jump button | 2.2 |
| DJ-03 | "Open in iTerm2" button shall send `POST /sessions/:name/jump` to the daemon | 2.2 |
| DJ-04 | The daemon shall spawn iTerm2 with the correct local or SSH command | 2.2 |
| DJ-05 | The modal shall display the exact shell command for reference | 2.2 |
| DJ-06 | The modal shall show success or failure feedback from the daemon response | 2.2 |
| DJ-07 | The modal shall be dismissible via Escape or clicking outside | 2.2 |

### 4.12 CLI (`scmux`)

| ID | Requirement | Sprint |
|----|-------------|--------|
| CLI-01 | `scmux` shall be a separate binary from `scmux-daemon` | 3.2 |
| CLI-02 | `scmux` shall communicate exclusively via the daemon HTTP API | 3.2 |
| CLI-03 | Default daemon URL: `http://localhost:7878`; override with `SCMUX_HOST` env var or `--host` flag | 3.2 |
| CLI-04 | `scmux list` — list sessions with status | 3.2 |
| CLI-05 | `scmux show <name>` — full session detail | 3.2 |
| CLI-06 | `scmux start <name>` — start session | 3.2 |
| CLI-07 | `scmux stop <name>` — stop session | 3.2 |
| CLI-08 | `scmux jump <name>` — launch terminal via daemon | 3.2 |
| CLI-09 | Project-definition create/edit/delete shall be dashboard editor-only in the current release (CLI write commands deferred) | 6.0 |
| CLI-10 | `scmux hosts` — list hosts with reachability | 3.2 |
| CLI-11 | `scmux daemon status` — show daemon health | 3.2 |
| CLI-12 | Reserved for future non-editor write flows (requires explicit approval model) | 6.0 |
| CLI-13 | `scmux host add` — deferred; host persistence remains editor-only in current release | 6.0 |
| CLI-14 | `scmux daemon restart` — deferred to v2. Not implemented in current release | — |

### 4.13 Session Registry

| ID | Requirement | Sprint |
|----|-------------|--------|
| SR-01 | Project definitions shall be stored in SQLite with: name, project, host_id, config_json, scheduling fields, enabled flag, repo metadata | 6.0 |
| SR-02 | `config_json` shall define pane-level agent launch metadata (`name`, `command`, `atm_agent`, `atm_team`), repo path, and optional layout | 6.0 |
| SR-03 | Session names shall be unique per host | 1.2 |
| SR-04 | Sessions shall be soft-deletable via `enabled = 0` | 1.2 |
| SR-05 | `cron_schedule` shall use standard 5-field cron format; NULL = manual-only | 1.2 |
| SR-06 | `github_repo` format: `owner/repo` (e.g. `randlee/scmux`) | 1.2 |
| SR-07 | Runtime discovery data (tmux sessions, pane activity, CI runs, ATM state) shall not be auto-persisted as project definitions | 6.0 |
| SR-08 | Permanent ATM roster/team composition edits shall be user-approved and persisted only via the editor write path | 6.0 |

### 4.14 Dashboard Embed & Self-Contained Binary (v0.4.0 / P5.1)

| ID | Requirement | Sprint |
|----|-------------|--------|
| DE-01 | The daemon shall embed the dashboard JS asset at compile time (via `include_str!` or `rust-embed`) so no runtime file dependencies are required | P5.1 |
| DE-02 | `GET /` shall serve the embedded `index.html` | P5.1 |
| DE-03 | `GET /dashboard.js` shall serve the pre-compiled dashboard JavaScript (JSX transpiled to React.createElement calls; no Babel in browser) | P5.1 |
| DE-04 | The `SCMUX_DASHBOARD_DIR` env var, when set, shall override embedded assets and serve files from disk (development mode) | P5.1 |
| DE-05 | The dashboard shall poll `/sessions`, `/hosts`, and `/dashboard-config.json` to populate live data | P5.1 |
| DE-06 | The dashboard visual design shall match the reference design: dark background (#060810), monospace font, per-project color bars, pulsing status dots, grid/list/grouped views, click-to-open jump modal with WezTerm button | P5.1 |
| DE-07 | `cargo package` shall succeed with embedded assets; `cargo install scmux-daemon` on a clean machine shall serve the dashboard | P5.1 |
| DE-08 | Both `scmux` and `scmux-daemon` crates shall be published to crates.io on each release tag | P5.1 |

### 4.15 Release Automation (v0.4.0 / P5.2)

| ID | Requirement | Sprint |
|----|-------------|--------|
| RA-01 | The release workflow shall automatically update the `randlee/homebrew-tap` Formula after each release tag, patching version, tarball URLs, and SHA256 checksums | P5.2 |
| RA-02 | The Homebrew formula update shall complete within 5 minutes of the release tag being pushed | P5.2 |
| RA-03 | No manual intervention shall be required for the Homebrew formula update after tagging | P5.2 |

### 4.16 ATM Integration — Agent Activity Monitoring (v0.4.0 / P5.3)

| ID | Requirement | Sprint |
|----|-------------|--------|
| ATM-01 | The daemon shall query the local ATM daemon via Unix socket IPC (`${ATM_HOME}/.claude/daemon/atm-daemon.sock`, overridable via `scmux.toml atm.socket_path`) for per-agent state | P5.3 |
| ATM-02 | ATM state shall be mapped per configured pane (`atm_agent`, `atm_team`) and exposed in session/project responses | 6.0 |
| ATM-03 | Dashboard and CLI shall render per-pane ATM state (`active|idle|stuck|offline|unknown`) rather than only a session-level badge | 6.0 |
| ATM-04 | `stuck` shall be derived from prolonged active state over configurable threshold (`atm.stuck_minutes`) | P5.3 |
| ATM-05 | ATM unavailability shall degrade gracefully: project remains visible, ATM fields marked unavailable/unknown, no daemon crash | P5.3 |
| ATM-06 | Runtime ATM observations shall not be auto-persisted as project-definition writes | 6.0 |
| ATM-07 | Canonical per-pane ATM lookup key shall be (`pane.atm_team`, `pane.atm_agent`); pane index/name are display metadata only | 6.0 |

---

## 5. Non-Functional Requirements

| ID | Requirement | Sprint |
|----|-------------|--------|
| NF-01 | The daemon binary shall be self-contained (no runtime deps beyond tmux, tmuxp, gh/az) | 4.1 |
| NF-02 | The daemon shall use < 50MB RAM in normal operation | 4.1 |
| NF-03 | Poll cycle shall complete in < 500ms for up to 50 projects | 4.1 |
| NF-04 | HTTP read endpoints shall respond in < 100ms | 4.1 |
| NF-05 | The system shall work on macOS (primary) and Linux (DGX Spark) | 4.1 |
| NF-07 | All CI/ATM/network errors shall be handled gracefully and logged with trace context | 6.0 |
| NF-08 | Single-host or single-session failures shall not crash the daemon or stop unrelated sessions | 4.1 |
| NF-09 | Stop behavior shall be graceful-first and must not perform bulk kill on panic/error paths | 6.0 |
| NF-10 | Logging/event schema shall remain OpenTelemetry-compatible to enable near-term instrumentation rollout | 6.0 |

---

## 6. Test Requirements

### 6.1 Writer-Gate and Persistence Tests

| ID | Test | Sprint |
|----|------|--------|
| T-WG-01 | Only the project-definition editor path can mutate SQLite; all other modules are denied at compile boundary and runtime checks | 6.0 |
| T-WG-02 | Poller modules (`tmux_poller`, `hosts`, `ci`, `atm`) perform zero SQLite writes during runtime polling | 6.0 |
| T-WG-03 | Unapproved project write attempts are rejected with explicit error | 6.0 |
| T-WG-04 | Deleting SQLite and restarting does not reconstruct definitions from tmux discovery | 6.0 |

### 6.2 Lifecycle and Safety Tests

| ID | Test | Sprint |
|----|------|--------|
| T-LC-01 | `POST /sessions/:name/start` launches tmux session and pane commands from `config_json` | 6.0 |
| T-LC-02 | Session transitions `stopped -> starting -> running -> idle` follow deterministic criteria; `done` transition-in behavior is deferred pending lifecycle decision | 6.0 |
| T-LC-03 | `POST /sessions/:name/stop` sends ATM shutdown first, waits grace period, then performs scoped hard-stop only if needed | 6.0 |
| T-LC-04 | Panic/error in one session does not stop or tear down unrelated sessions | 6.0 |
| T-LC-05 | Closing iTerm does not stop tmux session or running agents | 6.0 |
| T-LC-06 | `starting` failures transition session back to `stopped` with structured error details | 6.0 |

### 6.3 API Tests

| ID | Test | Sprint |
|----|------|--------|
| T-A-01 | `GET /health` returns daemon and poller health | 1.2 |
| T-A-02 | `GET /sessions` returns all defined projects including stopped projects | 6.0 |
| T-A-03 | `GET /sessions/:name` includes config, per-pane ATM state, and CI snapshot | 6.0 |
| T-A-04 | `GET /sessions/:name` returns 404 for unknown project | 1.2 |
| T-A-05 | `POST /sessions/:name/start` returns ok true/false with actionable message | 6.0 |
| T-A-06 | `POST /sessions/:name/stop` returns ok true/false with graceful-stop diagnostics | 6.0 |
| T-A-07 | `POST /sessions/:name/jump` opens viewer terminal without affecting session lifecycle | 6.0 |
| T-A-08 | Editor endpoints (`POST/PATCH/DELETE /sessions`) are the only persistent-write API paths | 6.0 |
| T-A-09 | `GET /discovery` is read-only and does not mutate definitions | 6.0 |
| T-A-10 | `POST /sessions/:name/start` returns structured validation errors for malformed/missing `config_json` | 6.0 |

### 6.4 Dashboard Manual Tests

| ID | Test | Sprint |
|----|------|--------|
| T-UI-01 | Primary dashboard view shows all defined projects (running and stopped) | 6.0 |
| T-UI-02 | Secondary discovery tab shows raw tmux sessions not linked to definitions | 6.0 |
| T-UI-03 | Project cards show per-pane ATM state (active/idle/stuck/offline/unknown) | 6.0 |
| T-UI-04 | CI panel shows per-project PR list and run indicators (green/yellow/red/running) | 6.0 |
| T-UI-05 | Unreachable host sessions render monochrome and recover automatically on reconnect | 2.1 |
| T-UI-06 | Jump modal attaches viewer terminal and does not change agent run state | 6.0 |

### 6.5 Reliability and Observability Tests

| ID | Test | Sprint |
|----|------|--------|
| T-RB-01 | Single host failure does not degrade unrelated hosts/projects | 4.1 |
| T-RB-02 | Single session launch/stop failure does not cascade to other sessions | 6.0 |
| T-RB-03 | No panic/error path issues bulk stop for all sessions | 6.0 |
| T-RB-04 | Logs include correlation fields suitable for OpenTelemetry export mapping | 6.0 |
| T-RB-05 | Runtime polling refreshes live state continuously without requiring persistent runtime writes | 6.0 |

### 6.6 Dashboard Embed Tests (v0.4.0 / P5.1)

| ID | Test | Sprint |
|----|------|--------|
| T-DE-01 | `cargo package --no-verify` succeeds with no runtime file dependencies | P5.1 |
| T-DE-02 | `GET /` returns 200 with HTML containing `<div id="root">` | P5.1 |
| T-DE-03 | `GET /dashboard.js` returns 200 with JavaScript (no JSX syntax) | P5.1 |
| T-DE-04 | Dashboard loads in browser and renders session list from `/sessions` | P5.1 |
| T-DE-05 | `SCMUX_DASHBOARD_DIR=/path` causes daemon to serve files from disk instead of embedded | P5.1 |
| T-DE-06 | `cargo install --path crates/scmux-daemon` on a clean machine serves the dashboard | P5.1 |

### 6.7 ATM Integration Tests (v0.4.0 / P5.3+)

| ID | Test | Sprint |
|----|------|--------|
| T-ATM-01 | `GET /sessions` returns per-pane ATM state mapping for ATM-enrolled projects | 6.0 |
| T-ATM-02 | `stuck` derivation is threshold-driven and test-validated | 6.0 |
| T-ATM-03 | ATM daemon unreachable degrades gracefully without daemon crash | P5.3 |
| T-ATM-04 | Dashboard renders per-pane activity states; non-ATM panes show `unknown` fallback | 6.0 |

---

## 7. Acceptance Criteria

> Note: The acceptance criteria below are referenced in sprint specs as AC-01..AC-10.
> They are not formal requirement IDs — they are phase completion gates. Do not
> include them in requirement coverage tables.

The system is complete when:

1. `cargo build --release --workspace` succeeds with no warnings
2. Daemon survives 24 hours on macOS without crashing
3. All writer-gate, lifecycle, API, and reliability tests pass
4. Primary dashboard shows all defined projects (running and stopped) with per-pane ATM state
5. Jump via dashboard opens iTerm2 attached to correct session (viewer-only behavior)
6. `stop` performs graceful ATM shutdown attempt before scoped hard-stop escalation
7. Auto-start/cron only affect explicitly defined projects
8. Disconnecting VPN for a remote host produces no error dialogs; host shows monochrome
9. Reconnecting VPN restores full-color display within one poll cycle
10. Missing `gh`/`az` tools show degraded CI state with install guidance

**v0.4.0 additional gates (AC-11..AC-16):**

11. `http://localhost:7878/` shows the real scmux dashboard matching the reference design (project colors, status dots, views, click-to-open jump modal with terminal button)
12. `cargo install scmux-daemon` on a clean machine serves the embedded dashboard
13. Both `scmux` and `scmux-daemon` are published to crates.io on release tag
14. `SCMUX_DASHBOARD_DIR` override loads from disk for local dev
15. Pushing a release tag auto-updates the Homebrew formula within 5 minutes
16. ATM-enrolled sessions show agent state in dashboard and CLI; ATM unavailable degrades gracefully

---

## 8. Open Questions — Resolved

| # | Question | Decision |
|---|----------|----------|
| OQ-1 | Dashboard served separately or by daemon? | Daemon serves static files at `/`. Single binary. |
| OQ-2 | How does browser trigger terminal launch? | `POST /sessions/:name/jump` → daemon spawns iTerm2 via AppleScript. No URI schemes. |
| OQ-3 | PR data: daemon or dashboard? | Daemon fetches via `gh`/`az` CLI. Adaptive interval: 1min active, 5min idle. Missing tools show gracefully. |
| OQ-4 | Multi-host config location? | Hosts are user-defined via editor and persisted in SQLite. VPN gaps are normal — monochrome, no errors. |
| OQ-5 | `scmux` CLI scope? | Separate binary, HTTP client to daemon. Same API as web UI. Persistent writes are restricted to the project-definition editor path. |
| OQ-6 | Final `done` semantics and auto-teardown policy? | Pending product decision; keep default non-destructive behavior until explicitly approved. |
| OQ-7 | Stop escalation exact parameters (timeouts/retries/hard-stop policy)? | Pending product decision; keep graceful-first and scoped behavior mandatory. |
| OQ-8 | Runtime state persistence model? | P6 uses in-memory runtime projection; pollers do not persist runtime snapshots to SQLite. |
| OQ-9 | Secondary discovery endpoint path? | `GET /discovery`. |
