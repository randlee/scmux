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

| ID | Requirement |
|----|-------------|
| DG-01 | The daemon shall be a single self-contained binary (`scmux-daemon`) |
| DG-02 | The daemon shall own all SQLite writes; no other component writes to the database |
| DG-03 | The daemon shall serve the web dashboard as static files at `GET /` |
| DG-04 | The daemon shall load configuration from `~/.config/scmux/scmux.toml` at startup |
| DG-05 | The daemon shall seed the SQLite `hosts` table from `scmux.toml` on first run if the table is empty |
| DG-06 | The daemon shall apply SQLite schema migrations on every startup (idempotent) |
| DG-07 | The daemon shall log structured output via `tracing` at INFO level by default |

### 4.2 Daemon — Session Lifecycle

| ID | Requirement |
|----|-------------|
| SL-01 | The daemon shall poll `tmux list-sessions` every 15 seconds |
| SL-02 | The daemon shall update `session_status` for all enabled sessions on every poll |
| SL-03 | A session not found in `tmux list-sessions` shall be marked `stopped` |
| SL-04 | A session found in `tmux list-sessions` shall be marked `running` |
| SL-05 | When a running session disappears between polls, the daemon shall write a `stopped` event to `session_events` |
| SL-06 | Sessions with `auto_start = 1` that are stopped shall be started by the daemon on the next poll cycle |
| SL-07 | Sessions with a `cron_schedule` shall be started when the cron expression fires, if currently stopped |
| SL-08 | Cron evaluation shall use a 15-second window to avoid missing fires between poll cycles |
| SL-09 | Sessions shall be started via `tmuxp load -d <temp_config_file>` |
| SL-10 | If `tmuxp load` fails, the daemon shall write a `failed` event with the error message to `session_events` |
| SL-11 | Failed starts shall not be retried in the same poll cycle |

### 4.3 Daemon — Pane Status

| ID | Requirement |
|----|-------------|
| PS-01 | The daemon shall collect pane info for each running session via `tmux list-panes` |
| PS-02 | Each pane shall report: index, name (pane_title), current_command, active flag |
| PS-03 | A pane with `pane_active = 1` shall be reported as status `active` |
| PS-04 | All other panes in a running session shall be reported as `idle` |
| PS-05 | If `pane_title` is empty, fall back to `pane-<index>` |
| PS-06 | Pane data shall be stored as JSON in `session_status.panes_json` |

### 4.4 Daemon — Terminal Launch (Jump)

| ID | Requirement |
|----|-------------|
| TL-01 | The daemon shall handle `POST /sessions/:name/jump` and spawn the terminal process |
| TL-02 | The browser shall never launch terminal processes directly |
| TL-03 | For MVP, iTerm2 shall be the supported terminal, launched via AppleScript |
| TL-04 | For a local session, the command shall be: `tmux attach -t <name>` |
| TL-05 | For a remote session, the command shall be: `ssh <user>@<host> tmux attach -t <name>` |
| TL-06 | The jump endpoint shall return `{ ok, message }` indicating success or failure |
| TL-07 | The default terminal shall be configurable in `scmux.toml` |
| TL-08 | A `terminal` field in the jump request body may override the default |

### 4.5 Daemon — CI Integration

| ID | Requirement |
|----|-------------|
| CI-01 | The daemon shall support two CI providers: GitHub (`gh` CLI) and Azure DevOps (`az` CLI) |
| CI-02 | Both providers are optional; sessions may have one, both, or neither |
| CI-03 | On startup, the daemon shall detect whether `gh` and `az` are available in PATH |
| CI-04 | If a required CLI tool is not available, the daemon shall record `tool_unavailable` status in `session_ci` with an install message |
| CI-05 | The daemon shall poll CI status on an adaptive interval per session |
| CI-06 | When any pane in a session has status `active`, the CI poll interval shall be 1 minute |
| CI-07 | When all panes are idle or session is stopped, the CI poll interval shall be 5 minutes |
| CI-08 | Each session shall track its own `next_poll_at` in `session_ci` |
| CI-09 | GitHub polling shall collect: open PRs (number, title, URL, author, draft flag) and recent workflow runs (status, branch, timestamp) via `gh pr list` and `gh run list` |
| CI-10 | Azure polling shall collect: open PRs and pipeline run status via `az pipelines` |
| CI-11 | CI results shall be stored as JSON blobs in `session_ci` with provider, polled_at, next_poll_at |
| CI-12 | The `github_repo` column on `sessions` shall hold the repo in `owner/repo` format |
| CI-13 | The `azure_project` column on `sessions` shall hold the Azure DevOps project URL or identifier |

### 4.6 Daemon — Health

| ID | Requirement |
|----|-------------|
| DH-01 | The daemon shall write a heartbeat row to `daemon_health` every 60 seconds |
| DH-02 | Each heartbeat shall include: host_id, status ("ok"), running session count, timestamp |
| DH-03 | The daemon shall prune `daemon_health` rows older than 7 days on each write |
| DH-04 | The daemon shall start automatically on machine boot (launchd / systemd) |
| DH-05 | The daemon shall restart automatically if it crashes |

### 4.7 HTTP API

| ID | Requirement |
|----|-------------|
| API-01 | The daemon shall expose HTTP on a configurable port (default 7700) |
| API-02 | All responses shall be JSON |
| API-03 | CORS shall be permissive |
| API-04 | `GET /health` — daemon status, host_id, running count, timestamp |
| API-05 | `GET /sessions` — all enabled sessions with live status, panes, CI summary |
| API-06 | `GET /sessions/:name` — full detail: config, panes, CI, last 20 events |
| API-07 | `GET /sessions/:name` — 404 if not found |
| API-08 | `POST /sessions/:name/start` — start session, log event, return ok/error |
| API-09 | `POST /sessions/:name/stop` — stop session, log event, return ok/error |
| API-10 | `POST /sessions/:name/jump` — spawn terminal, return ok/error |
| API-11 | `POST /sessions` — register new session (from CLI or UI) |
| API-12 | `PATCH /sessions/:name` — update session fields |
| API-13 | `DELETE /sessions/:name` — disable or remove session |
| API-14 | `GET /hosts` — list all hosts with reachability status |
| API-15 | `GET /dashboard-config.json` — host list + dashboard settings for web UI |
| API-16 | Start, stop, and jump actions shall be logged to `session_events` |

### 4.8 Multi-Host / VPN Handling

| ID | Requirement |
|----|-------------|
| MH-01 | Hosts shall be defined in `scmux.toml` and seeded into SQLite on first run |
| MH-02 | The dashboard shall poll all known hosts independently |
| MH-03 | A host that is unreachable (timeout, network error) shall NOT be treated as an error condition |
| MH-04 | When a host is unreachable, the daemon shall retain the last known session data |
| MH-05 | Unreachable hosts shall be flagged `reachable: false` in the `/hosts` response |
| MH-06 | The dashboard shall render sessions from unreachable hosts in monochrome (grayscale) |
| MH-07 | The dashboard shall display a "last seen N ago" indicator per host when unreachable |
| MH-08 | When a host becomes reachable again, the dashboard shall resume full-color rendering automatically |
| MH-09 | No error dialogs or alerts shall be shown for VPN-gated host unavailability |

### 4.9 Dashboard — Views and Filtering

| ID | Requirement |
|----|-------------|
| DV-01 | The dashboard shall support three views: Grid, List, Grouped-by-project |
| DV-02 | Grid: one card per session — name, project, status dot, pane list, CI/PR badges |
| DV-03 | List: table — name, project, status, pane count, active count, open PRs, last activity |
| DV-04 | Grouped: sessions under project headers with per-project running count and PR count |
| DV-05 | Stopped sessions shall be visually de-emphasized (reduced opacity) |
| DV-06 | Unreachable-host sessions shall be rendered in monochrome |
| DV-07 | Filter by status: all / running / idle / stopped |
| DV-08 | Filter by project |
| DV-09 | Text search by session name |
| DV-10 | Filters shall be combinable |
| DV-11 | Header shall show global counts: running, idle, stopped, active agents, open PRs |

### 4.10 Dashboard — CI Display

| ID | Requirement |
|----|-------------|
| DC-01 | Each session card shall show CI badges for configured providers |
| DC-02 | GitHub: show open PR count badge; click expands PR list with links |
| DC-03 | Azure: show pipeline status badge (passing / failing / running) |
| DC-04 | When a tool is unavailable, show a grayed badge with tooltip: "Install gh CLI: brew install gh" |
| DC-05 | When a session has no CI configured, show nothing (no empty badge) |

### 4.11 Dashboard — Jump

| ID | Requirement |
|----|-------------|
| DJ-01 | Clicking a session card shall open a jump modal |
| DJ-02 | The modal shall show: name, project, pane list with status, CI/PR details, jump button |
| DJ-03 | "Open in iTerm2" button shall send `POST /sessions/:name/jump` to the daemon |
| DJ-04 | The daemon shall spawn iTerm2 with the correct local or SSH command |
| DJ-05 | The modal shall display the exact shell command for reference |
| DJ-06 | The modal shall show success or failure feedback from the daemon response |
| DJ-07 | The modal shall be dismissible via Escape or clicking outside |

### 4.12 CLI (`scmux`)

| ID | Requirement |
|----|-------------|
| CLI-01 | `scmux` shall be a separate binary from `scmux-daemon` |
| CLI-02 | `scmux` shall communicate exclusively via the daemon HTTP API |
| CLI-03 | Default daemon URL: `http://localhost:7700`; override with `SCMUX_HOST` env var or `--host` flag |
| CLI-04 | `scmux list` — list sessions with status |
| CLI-05 | `scmux show <name>` — full session detail |
| CLI-06 | `scmux start <name>` — start session |
| CLI-07 | `scmux stop <name>` — stop session |
| CLI-08 | `scmux jump <name>` — launch terminal via daemon |
| CLI-09 | `scmux add --name --project --config --auto-start` — register session |
| CLI-10 | `scmux edit <name> [--cron] [--auto-start] [--config]` — update session |
| CLI-11 | `scmux disable <name>` / `scmux enable <name>` — toggle enabled flag |
| CLI-12 | `scmux remove <name>` — delete session |
| CLI-13 | `scmux hosts` — list hosts with reachability |
| CLI-14 | `scmux daemon status` — show daemon health |

### 4.13 Session Registry

| ID | Requirement |
|----|-------------|
| SR-01 | Sessions shall be stored in SQLite with: name, project, host_id, config_json, cron_schedule, auto_start, enabled, github_repo, azure_project |
| SR-02 | `config_json` shall be a valid tmuxp JSON blob with `session_name` matching the session `name` |
| SR-03 | Session names shall be unique per host |
| SR-04 | Sessions shall be soft-deletable via `enabled = 0` |
| SR-05 | `cron_schedule` shall use standard 5-field cron format; NULL = manual-only |
| SR-06 | `github_repo` format: `owner/repo` (e.g. `randlee/scmux`) |

---

## 5. Non-Functional Requirements

| ID | Requirement |
|----|-------------|
| NF-01 | The daemon binary shall be self-contained (no runtime deps beyond tmux, tmuxp, gh/az) |
| NF-02 | The daemon shall use < 50MB RAM in normal operation |
| NF-03 | Poll cycle shall complete in < 500ms for up to 50 sessions |
| NF-04 | HTTP read endpoints shall respond in < 100ms |
| NF-05 | The system shall work on macOS (primary) and Linux (DGX Spark) |
| NF-06 | The SQLite database shall be reconstructible from live tmux state on next poll if lost |
| NF-07 | All CI errors (network failure, auth error, rate limit) shall be handled gracefully and logged |
| NF-08 | The daemon shall not crash on any single-host or single-session failure |

---

## 6. Test Requirements

### 6.1 Daemon Unit Tests

| ID | Test |
|----|------|
| T-D-01 | `db::open()` creates schema on fresh database |
| T-D-02 | `db::open()` is idempotent on existing database |
| T-D-03 | `db::ensure_local_host()` inserts local host if absent |
| T-D-04 | `db::ensure_local_host()` returns existing host_id if present |
| T-D-05 | `should_run_now()` true when cron fires in 15s window |
| T-D-06 | `should_run_now()` false when cron does not fire in window |
| T-D-07 | `should_run_now()` false for invalid cron expression |
| T-D-08 | `tmux::live_sessions()` returns empty map when tmux not running |
| T-D-09 | `tmux::live_sessions()` parses session names correctly |
| T-D-10 | CI interval is 1 minute when any pane is active |
| T-D-11 | CI interval is 5 minutes when all panes are idle |
| T-D-12 | `tool_unavailable` recorded when `gh` not in PATH |
| T-D-13 | `tool_unavailable` recorded when `az` not in PATH |

### 6.2 Daemon Integration Tests

| ID | Test |
|----|------|
| T-I-01 | Poll cycle with no sessions completes without error |
| T-I-02 | Poll cycle marks session running when found in tmux |
| T-I-03 | Poll cycle marks session stopped when not found in tmux |
| T-I-04 | Poll cycle writes stopped event when session disappears |
| T-I-05 | Poll cycle starts auto_start session when stopped |
| T-I-06 | Poll cycle does not start disabled session |
| T-I-07 | Poll cycle does not restart already-running auto_start session |
| T-I-08 | Health write inserts daemon_health row |
| T-I-09 | Health write prunes rows older than 7 days |
| T-I-10 | Unreachable remote host does not crash poll cycle |
| T-I-11 | Host marked unreachable when /health times out |
| T-I-12 | Host resumes reachable when /health responds again |

### 6.3 API Tests

| ID | Test |
|----|------|
| T-A-01 | GET /health returns 200 with correct fields |
| T-A-02 | GET /sessions returns empty array when no sessions |
| T-A-03 | GET /sessions returns sessions with correct status and panes |
| T-A-04 | GET /sessions/:name returns 200 with config and events |
| T-A-05 | GET /sessions/:name returns 404 for unknown session |
| T-A-06 | POST /sessions/:name/start returns ok:true and logs event |
| T-A-07 | POST /sessions/:name/start returns ok:false on tmuxp failure |
| T-A-08 | POST /sessions/:name/stop returns ok:true and logs event |
| T-A-09 | POST /sessions/:name/jump returns ok:true when iTerm2 launched |
| T-A-10 | POST /sessions/:name/jump returns ok:false when terminal unavailable |
| T-A-11 | POST /sessions (add) creates session in SQLite |
| T-A-12 | PATCH /sessions/:name updates cron_schedule |
| T-A-13 | DELETE /sessions/:name disables session |
| T-A-14 | GET /hosts returns all hosts with reachability flag |

### 6.4 Dashboard Manual Tests

| ID | Test |
|----|------|
| T-UI-01 | Grid view renders all sessions |
| T-UI-02 | List view renders all sessions in table |
| T-UI-03 | Grouped view groups by project |
| T-UI-04 | Status filters work correctly |
| T-UI-05 | Project filter shows only correct sessions |
| T-UI-06 | Search filters by name substring |
| T-UI-07 | Clicking session opens jump modal |
| T-UI-08 | Modal shows correct pane list |
| T-UI-09 | Modal shows correct PR badges with links |
| T-UI-10 | Modal "Open in iTerm2" sends POST /jump and shows feedback |
| T-UI-11 | Stopped sessions are visually de-emphasized |
| T-UI-12 | Unreachable host sessions render in monochrome |
| T-UI-13 | "Last seen N ago" shows for unreachable hosts |
| T-UI-14 | Full color resumes when host returns |
| T-UI-15 | Tool-unavailable CI badges show tooltip with install instructions |
| T-UI-16 | Escape key closes modal |
| T-UI-17 | Header counts match data |

### 6.5 End-to-End Tests

| ID | Test |
|----|------|
| T-E-01 | Daemon starts, creates DB, serves /health |
| T-E-02 | Add session → daemon starts it via auto_start within 15s |
| T-E-03 | Kill session externally → daemon detects stopped within 15s |
| T-E-04 | POST /start → session appears in tmux |
| T-E-05 | POST /stop → session disappears from tmux |
| T-E-06 | POST /jump → iTerm2 opens, attaches to correct session |
| T-E-07 | Dashboard loads → shows real data from daemon |
| T-E-08 | Disconnect from VPN → remote host goes monochrome, no error dialog |
| T-E-09 | Reconnect VPN → remote host resumes full color |
| T-E-10 | `scmux list` → matches dashboard data |
| T-E-11 | `scmux jump <name>` → iTerm2 opens via daemon |

---

## 7. Acceptance Criteria

The system is complete when:

1. `cargo build --release --workspace` succeeds with no warnings
2. Daemon survives 24 hours on macOS without crashing
3. All T-D, T-I, and T-A tests pass
4. Dashboard shows real live data from daemon
5. Jump via dashboard opens iTerm2 attached to correct session
6. `auto_start` session killed externally restarts within 30 seconds
7. Cron-scheduled session starts within 15 seconds of scheduled time
8. Disconnecting VPN for a remote host produces no error dialogs; host shows monochrome
9. Reconnecting VPN restores full-color display within one poll cycle
10. Missing `gh`/`az` tools show grayed badges with install tooltip

---

## 8. Open Questions — Resolved

| # | Question | Decision |
|---|----------|----------|
| OQ-1 | Dashboard served separately or by daemon? | Daemon serves static files at `/`. Single binary. |
| OQ-2 | How does browser trigger terminal launch? | `POST /sessions/:name/jump` → daemon spawns iTerm2 via AppleScript. No URI schemes. |
| OQ-3 | PR data: daemon or dashboard? | Daemon fetches via `gh`/`az` CLI. Adaptive interval: 1min active, 5min idle. Missing tools show gracefully. |
| OQ-4 | Multi-host config location? | `scmux.toml` seeds SQLite. Hosts monitored continuously. VPN gaps are normal — monochrome, no errors. |
| OQ-5 | `scmux` CLI scope? | Separate binary, HTTP client to daemon. Same API as web UI. Daemon is sole SQLite writer. |
