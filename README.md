# scmux

tmux session manager for multi-agent Claude Code teams.

## What it does

- **Declarative session configs** stored in SQLite (tmuxp JSON format)
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

## Architecture

See [docs/architecture.md](docs/architecture.md) for the full design including multi-host setup, jump flow, and roadmap.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SCMUX_DB` | `~/.config/scmux/scmux.db` | SQLite database path |
| `SCMUX_PORT` | `7878` | HTTP API port |
