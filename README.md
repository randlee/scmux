# scmux

tmux session manager for multi-agent Claude Code teams.

## What it does

- **Declarative session configs** stored in SQLite (tmuxp JSON format)
- **Per-machine daemon** (`tms-daemon`) polls tmux, auto-starts scheduled sessions, serves HTTP status API
- **Web dashboard** shows all teams across all hosts — agent status, open PRs, jump-to-session via WezTerm

## Structure

```
scmux/
├── tms-daemon/          # Rust daemon (per machine)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs      # Entry point, task spawning
│       ├── db.rs        # SQLite helpers + migration
│       ├── tmux.rs      # tmux/tmuxp subprocess calls
│       ├── scheduler.rs # Poll cycle, cron, auto-start
│       └── api.rs       # axum HTTP handlers
├── dashboard/
│   ├── team-dashboard.jsx  # React dashboard (grid/list/grouped views)
│   └── README.md
└── docs/
    ├── architecture.md     # Full system design
    ├── schema.sql          # SQLite schema (reference)
    └── example-session.json
```

## Quick start

### Daemon

```bash
cd tms-daemon
cargo build --release
TMS_PORT=7700 ./target/release/tms-daemon
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
| `TMS_DB` | `~/.config/tms/tms.db` | SQLite database path |
| `TMS_PORT` | `7700` | HTTP API port |
