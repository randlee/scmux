# rmux — TOML-driven tmux Session Launcher

`rmux` reads an `[rmux]` section from `.atm.toml` and launches a fully configured tmux session with named windows and panes. It handles environment setup, agent identity, and team registration automatically.

---

## Installation

```bash
# From the scmux repo:
cp scripts/rmux ~/.local/bin/rmux && chmod +x ~/.local/bin/rmux
```

**Dependencies** (must be in PATH):
- `tmux`
- `jq`
- `python3` with `tomllib` (built-in ≥ 3.11) or `tomli` (`pip install tomli`)

---

## Commands

### `rmux` — Create session

Reads `.atm.toml` in the current directory and creates a tmux session with all configured windows and panes.

```bash
rmux                          # create session from .atm.toml in CWD
rmux --config ~/myproject/.atm.toml
rmux --dry-run                # preview without executing
rmux -v                       # verbose: log every tmux command
```

If the session already exists and you are **outside** tmux, it attaches. If you are **inside** tmux, it prints the attach command and exits.

---

### `rmux launch <pane-name>` — Re-launch a named pane

Finds a pane by name in the config and launches it into the running session. If the pane already exists (matched by tmux pane title), it re-sends the commands to that pane. If the pane is missing, it creates it in the correct window.

```bash
rmux launch team-lead
rmux launch quality-mgr --config /path/to/.atm.toml
rmux launch cobs --dry-run
```

Useful for restarting a crashed agent without rebuilding the entire session.

---

### `rmux <agent-type> <name>` — Spawn agent at runtime

Adds a new agent pane to the running session's `spare` window (created if it doesn't exist). Team and session are read from the local `.atm.toml`.

```bash
rmux codex   my-agent          # spawn codex --yolo pane named my-agent
rmux sonnet  my-agent          # spawn claude --model sonnet pane
rmux haiku   my-agent          # spawn claude --model haiku pane
rmux opus    my-agent          # spawn claude --model opus pane
rmux claude  my-agent          # alias for sonnet
rmux gemini  my-agent          # spawn gemini pane
```

**Options:**
```
--config <path>   Config file to read (default: .atm.toml in CWD)
--team <name>     Override ATM_TEAM
--model <name>    Override model for claude agents
--window <name>   Target window name (default: spare)
--dry-run         Preview without executing
```

**Examples:**
```bash
# From inside a schook pane — spawns into schook's spare window
rmux codex spare-dev

# Override team and window
rmux sonnet reviewer --team schook --window overflow

# Preview what would run
rmux haiku triage --dry-run
```

Agent identity is set via `ATM_IDENTITY=<name>` and `ATM_TEAM=<team>` env vars. Claude agents additionally receive `--agent-id`, `--agent-name`, `--team-name`, and `--parent-session-id` flags for team communication.

---

## `.atm.toml` Reference

### Minimal example

```toml
[rmux]
session = "myteam"

[[rmux.windows]]
name = "agents"
layout = "even-horizontal"

[[rmux.windows.panes]]
name = "team-lead"
model = "sonnet"
env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "myteam" }

[[rmux.windows.panes]]
name = "cdev"
command = "codex --yolo"
env = { ATM_IDENTITY = "cdev", ATM_TEAM = "myteam" }

[[rmux.windows.panes]]
name = "quality-mgr"
model = "sonnet"
env = { ATM_IDENTITY = "quality-mgr", ATM_TEAM = "myteam" }
```

### Full example — multi-team monorepo

```toml
[core]
default_team = "atm-dev"
identity = "team-lead"

[plugins.gh_monitor]
team = "atm-dev"
repo = "myorg/myrepo"
notify_target = ["quality-mgr@atm-dev", "team-lead@atm-dev"]
poll_interval_secs = 300

# ── rmux: tmux session launcher ──────────────────────────────────────
[rmux]
session = "atm-dev"

# ── Primary team window ───────────────────────────────────────────────
[[rmux.windows]]
name = "agents"
layout = "even-horizontal"

[[rmux.windows.panes]]
name = "team-lead"
model = "sonnet"
env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "atm-dev" }

[[rmux.windows.panes]]
name = "arch-ctm"
command = "codex --yolo"
env = { ATM_IDENTITY = "arch-ctm", ATM_TEAM = "atm-dev" }

[[rmux.windows.panes]]
name = "quality-mgr"
model = "sonnet"
env = { ATM_IDENTITY = "quality-mgr", ATM_TEAM = "atm-dev" }

# ── Monitoring window ─────────────────────────────────────────────────
[[rmux.windows]]
name = "monitoring"
layout = "even-vertical"

[[rmux.windows.panes]]
name = "logs"
command = "tail -f /tmp/atm.log"

[[rmux.windows.panes]]
name = "atm-monitor"
model = "haiku"
agent = "atm-monitor"

[[rmux.windows.panes]]
name = "atm-term"

# ── Sub-team: schook (separate team, separate repo) ───────────────────
[[rmux.windows]]
name = "schook"
layout = "even-horizontal"

[[rmux.windows.panes]]
name = "team-lead"
model = "sonnet"
dir = "/Users/me/github/schook"
env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "schook" }

[[rmux.windows.panes]]
name = "chook"
command = "codex --yolo"
dir = "/Users/me/github/schook"
env = { ATM_IDENTITY = "chook", ATM_TEAM = "schook" }

[[rmux.windows.panes]]
name = "quality-mgr"
model = "sonnet"
dir = "/Users/me/github/schook"
env = { ATM_IDENTITY = "quality-mgr", ATM_TEAM = "schook" }
```

### Pane field reference

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Pane display name and tmux pane title |
| `model` | string | Claude model: `sonnet`, `haiku`, `opus`. When set, launches claude |
| `command` | string | Raw shell command (used when `model` is not set) |
| `agent` | string | Named agent identity (overrides `ATM_IDENTITY` for team flags) |
| `prompt` | string | Appended as a positional arg to the claude command (e.g. a prompt file path) |
| `dir` | string | Working directory for the pane (defaults to config file directory) |
| `env` | table | Environment variables exported before the command runs |
| `color` | string | Reserved for agent color — flag name TBD |

### Window field reference

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Window name in tmux |
| `layout` | string | tmux layout: `even-horizontal`, `even-vertical`, `tiled`, `main-horizontal`, `main-vertical` |
| `panes` | array | List of pane definitions |

---

## How agent identity works

rmux distinguishes two roles based on `ATM_IDENTITY`:

**`team-lead`** — the primary agent that starts and owns the team. Gets no `--agent-id/--agent-name/--team-name` flags, which causes claude to create a new team session.

**All other claude panes** — secondary agents. Get the full trio of flags so they join the team-lead's session:
```
claude --model sonnet \
  --dangerously-skip-permissions \
  --teammate-mode tmux \
  --agent-id quality-mgr@schook \
  --agent-name quality-mgr \
  --team-name schook
```

**Codex and gemini panes** receive identity only via environment variables (`ATM_IDENTITY`, `ATM_TEAM`), not CLI flags.

---

## Environment setup per pane

For every pane, rmux runs this init sequence before the agent command:

```bash
cd <dir>
set -a; source .env; set +a; hash -r   # if .env exists
export ATM_IDENTITY=<value>             # from pane env table
export ATM_TEAM=<value>                 # from pane env table
```

The `.env` file is sourced from the config file's directory (or the `dir` field if set). Any secrets, API keys, or shared variables belong in `.env`.

---

## Setting up on a new machine

### 1. Clone scmux and install rmux

```bash
git clone https://github.com/randlee/scmux.git ~/github/scmux
cp ~/github/scmux/scripts/rmux ~/.local/bin/rmux
chmod +x ~/.local/bin/rmux
```

Ensure `~/.local/bin` is in your PATH:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### 2. Install dependencies

```bash
brew install tmux jq          # macOS
# or: sudo apt install tmux jq  (Ubuntu/Debian)

# Python tomllib is built-in with Python 3.11+
# For older Python:
pip install tomli
```

### 3. Clone your project repos

```bash
mkdir -p ~/github
git clone https://github.com/myorg/myrepo.git ~/github/myrepo
# ... repeat for each repo that has a sub-team window
```

### 4. Create or copy `.atm.toml`

Either copy from another machine or create from scratch. Update `dir` paths to match the new machine's layout.

```bash
# Option A: copy and edit
scp oldmachine:~/github/agent-team-mail/.atm.toml ~/github/agent-team-mail/
# then edit dir paths

# Option B: create fresh — see examples above
```

### 5. Create `.env` (if needed)

```bash
cat > ~/github/myrepo/.env <<'EOF'
ATM_TEAM=myteam
GITHUB_TOKEN=ghp_...
EOF
```

### 6. Launch

```bash
cd ~/github/myrepo
rmux              # creates the session
# or from anywhere:
rmux --config ~/github/myrepo/.atm.toml
```

Attach from outside tmux:
```bash
tmux attach -t myteam
```

---

## Tips

**Preview before launching:**
```bash
rmux --dry-run
rmux sonnet my-agent --dry-run
```

**Restart a crashed pane without rebuilding the session:**
```bash
rmux launch quality-mgr
```

**Add a temporary agent to investigate something:**
```bash
rmux sonnet investigator --window scratch
```

**Use a different config file:**
```bash
rmux --config /path/to/other/.atm.toml
rmux launch team-lead --config /path/to/other/.atm.toml
```

**Multiple teams on one machine:** Each team's `.atm.toml` defines its own `session` name, so sessions don't conflict. You can run them all simultaneously in the same tmux server.
