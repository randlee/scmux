# scmux-daemon

Per-machine daemon for [scmux](https://github.com/randlee/scmux) — tmux session manager for multi-agent Claude Code teams.

Owns: SQLite database, tmux session lifecycle, CI polling, terminal launch, HTTP API, and static file serving for the web dashboard.

## Usage

```bash
scmux-daemon
```

Listens on `http://localhost:7878` by default. Configure via `~/.config/scmux/scmux.toml`.

See the [main repository](https://github.com/randlee/scmux) for full documentation.
