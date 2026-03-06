# scmux — Architecture

## 1. Purpose

scmux solves a specific problem: when running 20–30 concurrent Claude Code agent teams across multiple machines and terminal emulators, there is no single place to see what is running, find a session by name, or jump to it instantly.

scmux provides:
- A **daemon** per machine — sole owner of SQLite, session lifecycle, CI polling, and terminal launch
- A **web dashboard** — browser UI, read/command only, talks to daemon via HTTP
- A **CLI** (`scmux`) — shell client, same HTTP API as the web UI
- **Graceful degradation** — missing tools, unreachable hosts, and VPN gaps are normal operating conditions, not errors

## 2. Core Design Principles

**Daemon is the single source of truth.** All state lives in SQLite. Only the daemon writes to SQLite. The CLI and web UI are pure clients — they send commands to the daemon and read responses. This keeps the system in sync regardless of how many clients are connected simultaneously.

**Browser → daemon → terminal.** The web UI never launches processes directly. A "jump" button click sends `POST /sessions/:name/jump` to the daemon, which spawns the terminal. This eliminates race conditions and stale state from multiple browser tabs.

**Unavailability is normal.** A host behind a VPN that goes offline is not an error — it is an expected state. The dashboard shows last known data in monochrome. When the host returns, it resumes full color automatically.

**Missing tools degrade gracefully.** If `gh` or `az` CLI are not installed, CI status shows "unavailable" with a tooltip explaining what to install. The rest of the system works normally.

**One daemon per user per machine.** The daemon runs as the logged-in OS user and only sees that user's tmux sessions — this is enforced naturally by tmux's user isolation. Multiple people can use the same remote machine simultaneously; each person runs their own daemon on their own port. There is no cross-user session visibility. The `api_port` field in host config is the differentiator. A user's local scmux config points to the specific port for their daemon on each remote machine.

**Dashboard → local daemon only.** The browser dashboard never contacts remote hosts directly. The local daemon polls remote daemons via HTTP (`GET remote:api_port/sessions`) and caches the results locally. When a remote host is unreachable, the local daemon serves last-known session data from its local cache. This decouples dashboard availability from remote host availability.

## 3. System Diagram

```
┌─ Mac Studio ────────────────────────────────────────────┐
│                                                         │
│  scmux-daemon  (HTTP :7700)                               │
│  ├── SQLite  ~/.config/scmux/scmux.db  (SSOT)              │
│  ├── poll_loop        every 15s  → tmux ls              │
│  ├── ci_loop          adaptive   → gh / az cli          │
│  ├── health_loop      every 60s  → heartbeat            │
│  └── axum HTTP server            → web + CLI clients    │
│                                                         │
│  Clients:                                               │
│  ├── Browser  →  dashboard (static files from daemon)   │
│  └── scmux CLI  →  same HTTP API                          │
│                                                         │
└──────────────────────────┬──────────────────────────────┘
                           │ HTTP :7700
┌─ DGX Spark ──────────────┼──────────────────────────────┐
│  scmux-daemon  (HTTP :7700)│                               │
│  SQLite  ~/.config/scmux/scmux.db                           │
│  ... (same structure)                                   │
└──────────────────────────┼──────────────────────────────┘
                           │
                  ┌────────┴────────┐
                  │  Browser        │
                  │  Dashboard      │
                  │  (polls all     │
                  │   known hosts)  │
                  └────────┬────────┘
                           │ POST /sessions/:name/jump
                           │
                  ┌────────▼────────┐
                  │  scmux-daemon     │
                  │  spawns iTerm2  │
                  │  returns status │
                  └─────────────────┘
```

## 4. Components

### 4.1 scmux-daemon

**Language:** Rust
**Binary:** `scmux-daemon`
**Owns:** SQLite database, tmux lifecycle, CI polling, terminal launch, HTTP server, static file serving

#### Internal task structure

```
main()
  ├── spawn: poll_loop        every 15s
  │     └── scheduler::poll_cycle()
  │           ├── tmux::live_sessions()
  │           ├── update session_status
  │           ├── detect stops → session_events
  │           ├── check auto_start → tmuxp load
  │           └── check cron_schedule → tmuxp load
  │
  ├── spawn: ci_loop          adaptive interval
  │     └── ci::poll_all_sessions()
  │           ├── for each session with github_repo → gh pr list / gh run list
  │           ├── for each session with azure_project → az pipelines list
  │           ├── write results to session_ci table
  │           └── adjust next interval based on agent activity
  │
  ├── spawn: health_loop      every 60s
  │     └── db::write_health()
  │
  └── axum::serve
        ├── GET  /                         → serve dashboard HTML
        ├── GET  /dashboard-config.json    → host list + settings
        ├── GET  /health
        ├── GET  /sessions
        ├── GET  /sessions/:name
        ├── POST /sessions/:name/start
        ├── POST /sessions/:name/stop
        └── POST /sessions/:name/jump      → spawn terminal
```

#### Shared state

```rust
pub struct AppState {
    pub db:      Mutex<rusqlite::Connection>,
    pub host_id: i64,
    pub config:  Config,   // loaded from scmux.toml at startup
}
```

Only the daemon writes to SQLite. HTTP handlers read from `session_status` (written by poll_loop). Start/stop/jump handlers call subprocess, then write events.

### 4.2 Jump Flow

The browser never launches terminals. The daemon does.

```
1. User clicks "Jump" in dashboard
2. Browser sends:  POST /sessions/ui-template/jump
                   Body: { "terminal": "iterm2" }
3. Daemon:
   a. Looks up session → host (local or remote)
   b. Constructs command:
      Local:  tmux attach -t ui-template
      Remote: ssh user@host tmux attach -t ui-template
   c. Spawns iTerm2 with that command (via AppleScript or open)
   d. Returns: { "ok": true, "message": "launched iTerm2" }
4. Dashboard shows success/failure toast
```

#### iTerm2 launch (macOS)

```bash
# Local session
osascript -e '
  tell application "iTerm2"
    create window with default profile
    tell current session of current window
      write text "tmux attach -t ui-template"
    end tell
  end tell
'

# Remote session
osascript -e '
  tell application "iTerm2"
    create window with default profile
    tell current session of current window
      write text "ssh user@dgx-spark tmux attach -t rust-imgproc"
    end tell
  end tell
'
```

This approach gives a clean new iTerm2 window with the session attached, no URI scheme required, no race conditions.

#### Future terminal support

WezTerm and Warp support added later via the same `POST /sessions/:name/jump` endpoint — just different spawn logic in the daemon. The `terminal` field in the request selects the handler. Default terminal configured in `scmux.toml`.

### 4.3 CI Integration

#### Providers

| Provider | CLI | Column | Status when missing |
|----------|-----|--------|-------------------|
| GitHub | `gh` | `github_repo` | "unavailable" + tooltip |
| Azure DevOps | `az` | `azure_project` | "unavailable" + tooltip |

Both are optional. Sessions can have one, both, or neither.

#### Adaptive polling interval

CI polling frequency adapts based on agent activity:

```
if any pane in session has status = "active":
    poll_interval = 1 minute   (agents working → PRs/pipelines changing)
else:
    poll_interval = 5 minutes  (idle → less frequent)
```

Each session tracks its own next poll time in `session_ci.next_poll_at`.

#### Data collected

Per session, per provider:
- Open PRs: number, title, URL, author, draft flag
- Pipeline/action runs: status (passing/failing/running), branch, timestamp

Stored in `session_ci` table as JSON blobs with `provider`, `polled_at`, `next_poll_at`.

#### Missing tool handling

On daemon startup, check for `gh` and `az` in PATH. Store availability in `AppState`. When a session has `github_repo` set but `gh` is unavailable, `session_ci` gets:

```json
{
  "provider": "github",
  "status": "tool_unavailable",
  "message": "Install gh CLI: brew install gh"
}
```

Dashboard renders this as a grayed badge with tooltip.

### 4.4 Multi-Host Management

#### Host discovery

Hosts are defined in `scmux.toml` (version-controlled, seeded into SQLite on first run). Once in SQLite, the daemon monitors them actively.

```toml
[daemon]
port = 7700
default_terminal = "iterm2"

[[hosts]]
name = "mac-studio"
address = "localhost"
is_local = true

[[hosts]]
name = "dgx-spark"
address = "192.168.1.50"
ssh_user = "randlee"
api_port = 7700
```

#### VPN-gated hosts

Hosts that go offline (VPN disconnect, machine sleep, network change) are **normal operating conditions**, not errors. The daemon handles this as follows:

```
poll remote host /health:
  success → update last_seen, show full color, merge fresh data
  timeout/error → do NOT update last_seen, keep last known data
                   mark host as "unreachable" in memory (not SQLite)

dashboard receives:
  host.reachable = false → render all sessions in monochrome
  host.reachable = true  → render in full color
  host.last_seen         → show "last seen 4m ago" in host header
```

No error dialogs. No red alerts. Monochrome = "we have stale data, host is away."

When the host returns (VPN reconnects, machine wakes), the next poll cycle picks it up automatically and resumes full color.

### 4.5 scmux CLI

**Binary:** `scmux` (separate from `scmux-daemon`)
**Talks to:** `scmux-daemon` HTTP API (same endpoints as web UI)

The CLI is a thin HTTP client. All business logic lives in the daemon.

```bash
# Session management
scmux list                          # all sessions + status
scmux list --project radiant-p3     # filtered
scmux show ui-template              # full detail + recent events
scmux start ui-template
scmux stop ui-template
scmux jump ui-template              # daemon launches iTerm2

# Session registration
scmux add --name ui-template \
        --project radiant-p3 \
        --config ./ui-template.json \
        --auto-start

scmux edit ui-template --cron "0 9 * * 1-5"
scmux disable ui-template
scmux enable ui-template
scmux remove ui-template

# Host management
scmux hosts                         # list hosts + reachability
scmux host add --name dgx-spark \
             --address 192.168.1.50 \
             --ssh-user randlee

# Daemon control
scmux daemon status
scmux daemon restart
```

CLI connects to daemon at `http://localhost:7700` by default. Override with `SCMUX_HOST` env var or `--host` flag for managing remote hosts directly.

### 4.6 Dashboard

**Served by:** `scmux-daemon` as static files at `/`
**Framework:** React (single JSX file for dev; Vite build for production embedding)

The dashboard is configuration-aware via `GET /dashboard-config.json`:

```json
{
  "hosts": [
    { "name": "mac-studio", "url": "http://localhost:7700",     "is_local": true },
    { "name": "dgx-spark",  "url": "http://192.168.1.50:7700", "is_local": false }
  ],
  "default_terminal": "iterm2",
  "poll_interval_ms": 15000
}
```

Dashboard polls each host's `/sessions` endpoint independently. Unreachable hosts render in monochrome with last-known data and a "last seen N ago" indicator.

## 5. SQLite Schema Summary

One database per machine at `~/.config/scmux/scmux.db`. Schema applied via migration on daemon startup.

| Table | Purpose | Written by |
|-------|---------|-----------|
| `hosts` | Known machines | daemon (seeded from scmux.toml) |
| `sessions` | Session definitions + configs | daemon (via CLI/API) |
| `session_status` | Live tmux state | poll_loop only |
| `session_events` | Immutable start/stop log | poll_loop + API handlers |
| `session_ci` | CI/PR status per session | ci_loop only |
| `daemon_health` | Heartbeat, 7-day retention | health_loop only |

**Only the daemon writes to SQLite.** CLI and web UI read via HTTP.

## 6. Configuration

`~/.config/scmux/scmux.toml` — loaded at daemon startup, seeds SQLite if tables are empty.

```toml
[daemon]
port = 7700
db_path = "~/.config/scmux/scmux.db"
default_terminal = "iterm2"
log_level = "info"

[polling]
tmux_interval_secs = 15
health_interval_secs = 60
ci_active_interval_secs = 60
ci_idle_interval_secs = 300

[[hosts]]
name    = "mac-studio"
address = "localhost"
is_local = true

[[hosts]]
name     = "dgx-spark"
address  = "192.168.1.50"
ssh_user = "randlee"
api_port = 7700
```

## 7. Process Supervision

The daemon is a long-running process. Supervision keeps it alive across reboots and crashes.

### macOS (launchd)

`~/Library/LaunchAgents/com.scmux.scmux-daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.scmux.scmux-daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/scmux-daemon</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/tmp/scmux-daemon.log</string>
  <key>StandardErrorPath</key><string>/tmp/scmux-daemon.err</string>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/com.scmux.scmux-daemon.plist
```

### Linux (systemd)

```ini
[Unit]
Description=scmux-daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/scmux-daemon
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```

## 8. Build Layout

```
scmux/
├── crates/
│   ├── scmux-daemon/      # Cargo workspace member — HTTP daemon + SQLite
│   └── scmux/             # Cargo workspace member — CLI client
├── dashboard/           # React source (built output embedded in scmux-daemon)
├── docs/
│   ├── architecture.md
│   ├── requirements.md
│   ├── schema.sql
│   └── example-session.json
├── Cargo.toml           # workspace root
└── scmux.toml.example     # reference config
```

```bash
# Build everything
cargo build --release --workspace

# Install
cp target/release/scmux-daemon /usr/local/bin/
cp target/release/scmux       /usr/local/bin/

# Run
scmux-daemon
```

## 9. Future Work

| Item | Notes |
|------|-------|
| WezTerm jump | AppleScript equivalent or `wezterm` CLI via daemon |
| Warp jump | Monitor for URI scheme support |
| Browser terminal | ttyd integration for remote sessions without local terminal |
| Dashboard build pipeline | Embed built React into scmux-daemon binary via `include_str!` |
| `scmux edit` TUI | Interactive session config editor |
| Azure DevOps deep integration | PR links, pipeline status, work items |
| Host auto-discovery | mDNS broadcast from daemon |
| Session templates | Parameterized configs for spinning up new teams quickly |
