# hmux — herdr-native ATM Team Launcher

`hmux` is the going-forward ATM team launcher for Synaptic Canvas. It reads the
same `[rmux]` section from `.atm.toml` as `rmux` and launches a fully configured
agent team — but into **herdr** (workspace/tab/pane) instead of tmux. It handles
environment setup, agent identity, and team registration automatically.

`rmux` (tmux backend) is retained for parity. `hmux` is a drop-in replacement:
same CLI surface, same `.atm.toml` config, cross-platform shell support (bash on
POSIX, pwsh on Windows).

---

## Installation

```bash
# From the scmux repo:
cp scripts/hmux scripts/hmux_core.py scripts/hmux_backend.py ~/.local/bin/
chmod +x ~/.local/bin/hmux
```

**Dependencies** (must be in PATH):
- `herdr` — the pane/workspace backend (instead of `tmux`)
- `python3` with `tomllib` (built-in ≥ 3.11) or `tomli` (`pip install tomli`)

---

## Commands

### `hmux` — Create session

Reads `.atm.toml` in the current directory and creates a herdr workspace (keyed
by `ATM_TEAM`) with all configured tabs and panes, then launches each agent.

```bash
hmux                            # launch session from .atm.toml in CWD
hmux --config ~/myproject/.atm.toml
hmux --dry-run                  # preview without executing
hmux -v                         # verbose
```

If a workspace with the team's label already exists, hmux reuses it (find-or-create).

---

### `hmux launch <pane-name>` — Re-launch a named pane

Finds a pane by `name` in the config, resolves its herdr pane label, and launches
it into the running workspace. If the pane is missing, it is created in the
correct tab.

```bash
hmux launch team-lead
hmux launch quality-mgr --config /path/to/.atm.toml
hmux launch cobs --dry-run
```

Useful for restarting a crashed agent without rebuilding the entire workspace.

---

### `hmux <agent-type> <name>` — Spawn agent at runtime

Adds a new agent pane to the workspace's `spare` tab (created if it doesn't
exist), registers it with `atm teams add-member`, and launches it.

```bash
hmux codex   my-agent          # spawn codex --yolo pane
hmux sonnet  my-agent          # spawn claude --model sonnet pane
hmux haiku   my-agent          # spawn claude --model haiku pane
hmux opus    my-agent          # spawn claude --model opus pane
hmux fable   my-agent          # spawn claude --model fable pane
hmux claude  my-agent          # alias for sonnet
hmux gemini  my-agent          # spawn gemini pane
hmux hermes  my-agent          # spawn hermes --profile pane
```

**Options:**

```
--config <path>   Config file to read (default: .atm.toml in CWD)
--team <name>     Override ATM_TEAM
--model <name>    Override model for claude-family and hermes agents
--window <name>   Target tab name (default: spare)
--shell <name>    Force shell dialect: bash | pwsh (default: auto / HMUX_SHELL)
--dry-run         Preview without executing
```

**Examples:**

```bash
# Spawn a codex agent into the team's spare tab
hmux codex spare-dev

# Override team and tab
hmux sonnet reviewer --team schook --window overflow

# Preview what would run
hmux haiku triage --dry-run
```

Agent identity is set via `ATM_IDENTITY=<name>` and `ATM_TEAM=<team>` env vars.
Claude-family agents additionally receive `--agent-id`, `--agent-name`, and
`--team-name` (and `--parent-session-id` when the team config provides a
`leadSessionId`).

---

## Cross-platform shell support

`hmux` emits shell commands in two dialects, so a single `.atm.toml` works on
both POSIX and Windows hosts:

| Dialect | Used on | Init / env loading | Quoting |
|---|---|---|---|
| `bash` | macOS, Linux | `cd <dir>; set -a; source .env; set +a; hash -r` | `shlex.quote` (POSIX) |
| `pwsh` | Windows | `cd <dir>; Get-Content .env \| ForEach-Object { ... }` | single-quote with `''` escaping |

Resolution order: explicit `--shell` → `HMUX_SHELL` env (`bash`/`posix`/`sh` or
`pwsh`/`powershell`) → platform (`os.name == "nt"` → pwsh, else bash).

---

## `.atm.toml` Reference

`hmux` reads `[rmux]` **verbatim** — no new config attributes. See `rmux.md` for
the full pane/window field reference and examples. A minimal team:

```toml
[rmux]
session = "myteam"          # read for compat; workspace keyed by ATM_TEAM

[[rmux.windows]]
name = "agents"
layout = "even-horizontal"  # read but IGNORED (no herdr analogue)

[[rmux.windows.panes]]
name = "team-lead"
model = "sonnet"
env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "myteam" }

[[rmux.windows.panes]]
name = "cdev"
command = "codex --yolo"
env = { ATM_IDENTITY = "cdev", ATM_TEAM = "myteam" }
```

### Mapping tmux → herdr

| tmux (rmux) | herdr (hmux) |
|---|---|
| session | workspace (keyed by `ATM_TEAM`, **not** `[rmux].session`) |
| window | tab |
| pane | pane (label = `ATM_IDENTITY`) |
| `layout` | ignored — splits alternate `right`/`down` deterministically |

### Pane identity — two distinct concepts

`hmux` keeps two separate identity resolutions (do not conflate them):

- **Trio identity** (`--agent-id/--agent-name/--team-name`, claude only) —
  faithful to rmux: `agent` field → `env.ATM_IDENTITY` (non-empty, ≠
  `team-lead`) → none (team-lead).
- **Pane label** (herdr `pane rename` + nudge lookup key) — always non-empty:
  `env.ATM_IDENTITY` → `agent` → `name`.

---

## Differences from rmux (enumerated deltas)

`hmux --dry-run` matches `rmux --dry-run` field-for-field **except**:

1. Claude commands drop `--teammate-mode tmux` (tmux integration does not apply
   under herdr). The undocumented trio `--agent-id/--agent-name/--team-name` is
   kept.
2. The workspace key is `ATM_TEAM`; `[rmux].session` is read but ignored.
3. Pane ids are herdr `wN:pN`, where rmux reports tmux `%N`.

Anything not in that list must match. Full design record: see `hmux-plan.md`.

---

## Nudge path (herdr backend)

`atm send <agent>` wakes the target pane via `herdr pane run` rather than
`tmux send-keys`. The `atm-nudge-xml-1.py` script selects the herdr backend when
`herdr` is on PATH and `herdr pane list --workspace <ATM_TEAM>` succeeds;
otherwise it falls back to the tmux path unchanged.

---

## Setting up on a new machine

```bash
# 1. Clone scmux and install hmux
git clone https://github.com/randlee/scmux.git ~/github/scmux
cp ~/github/scmux/scripts/hmux ~/github/scmux/scripts/hmux_core.py \
   ~/github/scmux/scripts/hmux_backend.py ~/.local/bin/
chmod +x ~/.local/bin/hmux

# 2. Install dependencies
brew install herdr          # macOS (or the herdr install path for your OS)
# Python tomllib is built-in with Python 3.11+; for older Python: pip install tomli

# 3. Create or copy .atm.toml + .env (see rmux.md)

# 4. Launch
cd ~/github/myrepo
hmux                        # creates the herdr workspace
```

---

## Tips

**Preview before launching:**

```bash
hmux --dry-run
hmux sonnet my-agent --dry-run
```

**Restart a crashed pane without rebuilding the workspace:**

```bash
hmux launch quality-mgr
```

**Force a shell dialect (e.g. on a mixed machine):**

```bash
hmux --shell pwsh
HMUX_SHELL=bash hmux
```

**Add a temporary agent to investigate something:**

```bash
hmux sonnet investigator --window scratch
```
