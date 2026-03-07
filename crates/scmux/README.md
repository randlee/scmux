# scmux

CLI client for [scmux](https://github.com/randlee/scmux) — tmux session manager for multi-agent Claude Code teams.

Thin HTTP client that talks to `scmux-daemon`. All business logic lives in the daemon.

## Usage

```bash
scmux list                    # all sessions + status
scmux show <name>             # full detail
scmux start <name>
scmux stop <name>
scmux jump <name>             # daemon launches iTerm2
```

Connects to `http://localhost:7878` by default. Override with `SCMUX_HOST` env var or `--host` flag.

See the [main repository](https://github.com/randlee/scmux) for full documentation.
