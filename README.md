# scmux

tmux session manager for multi-agent Claude Code teams.

## What it does

- **Declarative session configs** stored in SQLite (tmuxp JSON by default,
  with an optional external-launcher adapter)
- **Per-machine daemon** (`scmux-daemon`) polls tmux, auto-starts scheduled sessions, serves HTTP status API
- **Web dashboard** shows all teams across all hosts — agent status, open PRs, jump-to-session via iTerm2

## Structure

```
scmux/
├── crates/
│   ├── scmux-daemon/      # Rust daemon (per machine)
│   └── scmux/             # CLI client
├── dashboard/
│   ├── team-dashboard.jsx  # React dashboard (grid/list/grouped views)
│   └── README.md
└── docs/
    ├── architecture.md     # Full system design
    ├── deploy.md           # launchd/systemd setup
    ├── schema.sql          # SQLite schema (reference)
    └── example-session.json
```

## Quick start

### Daemon

```bash
cd crates/scmux-daemon
cargo build --release
SCMUX_PORT=7878 ./target/release/scmux-daemon
```

### Dashboard

```bash
cd dashboard
npm create vite@latest . -- --template react
# paste team-dashboard.jsx into src/App.jsx
npm run dev
```

### Add a session

```sql
INSERT INTO sessions (name, project, host_id, config_json, auto_start)
VALUES (
  'ui-template',
  'radiant-p3',
  1,
  '{ ... tmuxp JSON ... }',
  1
);
```

See `docs/example-session.json` for a full tmuxp config example.

### External launchers

Teams that must perform lifecycle work beyond tmux creation can keep scmux as
their dashboard and launch surface while delegating startup to an argv-based
adapter:

```json
{
  "session_name": "aidw-platform",
  "start_directory": "~/git-checkouts/AIDevWorkspace",
  "launch": {
    "command": [
      "scripts/aidw",
      "--config-dir",
      "config/aidw-platform",
      "team",
      "start"
    ]
  },
  "panes": []
}
```

`launch.command` is an argv array and never a shell string. `{team_name}` may
be used in an argument. The command runs from `launch.working_directory`,
`root_path`, or `start_directory` (in that order), with home-directory
expansion on macOS/Linux and Windows. Definitions without `launch.command`
continue to start through tmuxp.

The dashboard labels external sessions as `Launch`/`Managed`. Their stop
lifecycle stays with the external launcher; scmux refuses a direct stop rather
than bypassing launcher-owned cleanup or registration.

### Host-qualified jump names

Dashboard jump actions accept host routing in the registered team name. Use
`<session>@local` for a session on the dashboard computer, or
`<session>@<ssh-user>@<computer-id>` for a session that may live on another
computer. For example, `aidw-dev@aidwlead@HITL2` attaches directly when the
dashboard runs on `HITL2`; otherwise it opens
`ssh aidwlead@HITL2 tmux attach -t aidw-dev`. SSH key exchange must already be
configured. Existing unqualified names continue to use their scmux host record.

## Architecture

See [docs/architecture.md](docs/architecture.md) for the full design including multi-host setup, jump flow, and roadmap.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SCMUX_DB` | `~/.config/scmux/scmux.db` | SQLite database path |
| `SCMUX_PORT` | `7878` | HTTP API port |
