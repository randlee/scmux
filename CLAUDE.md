# Claude Instructions for scmux

## ⚠️ CRITICAL: Branch Management Rules

**NEVER switch the main repository branch on disk from `develop`.**

- The main repo MUST remain on `develop` at all times
- **ALWAYS use `sc-git-worktree` skill** to create worktrees for all development work
- **ALWAYS create worktrees FROM `develop` branch** (not from `main`)
- Do NOT use `git checkout` or `git switch` in the main repository
- All sprint work happens in worktrees at `../scmux-worktrees/<branch-name>`
- **All PRs target `develop` branch** (integration branch, not `main`)

**Why**: Switching branches in the main repo breaks worktree references and destabilizes the development environment.

**Worktree Creation Pattern**:
```bash
# ✅ CORRECT: Create worktree from develop
/sc-git-worktree --create feature/s0-workspace-scaffold develop

# ❌ WRONG: Creating from main
/sc-git-worktree --create feature/s0-workspace-scaffold main
```

---

## Project Overview

**scmux** is a tmux session manager for multi-agent Claude Code teams:
- Per-machine Rust daemon (`scmux-daemon`) — sole owner of SQLite, session lifecycle, CI polling, terminal launch, HTTP API
- React web dashboard — read/command only, polls daemon via HTTP
- Rust CLI (`scmux`) — thin HTTP client, same API as web UI
- Graceful degradation — missing tools, unreachable hosts, VPN gaps are normal operating conditions

**Goal**: A single place to see all running agent sessions across all machines, jump to any session in ≤2 clicks, and auto-start/schedule sessions without manual intervention.

---

## Project Plan

**Docs**: [`docs/architecture.md`](./docs/architecture.md) · [`docs/requirements.md`](./docs/requirements.md)

**Sprint plan** (tracked via task list `scmux`):

| Sprint | Focus |
|--------|-------|
| S0 | Fix cargo check, workspace scaffold, config loader, DB migrations |
| S1 | Session lifecycle, missing API endpoints, event logging |
| S2 | iTerm2 jump, multi-host reachability, live dashboard |
| S3 | CI integration (gh/az), scmux CLI binary |
| S4 | Hardening, launchd/systemd, E2E acceptance tests |

---

## Key Documentation

- [`docs/architecture.md`](./docs/architecture.md) — System design, component diagram, jump flow, CI integration, multi-host
- [`docs/requirements.md`](./docs/requirements.md) — Full functional/non-functional requirements and test matrix
- [`docs/schema.sql`](./docs/schema.sql) — SQLite schema reference
- [`docs/example-session.json`](./docs/example-session.json) — tmuxp config example

---

## Workflow

### Sprint Execution Pattern

Every sprint follows this pattern:

1. **Create worktree** using `sc-git-worktree` skill from `develop`
2. **Implementation** by assigned agent(s)
3. **Tests pass** — `cargo check --workspace` clean, unit/integration tests green
4. **Commit/Push/PR** targeting `develop`
5. **Review and merge**

### Branch Flow

- Sprint PRs → `develop` (integration branch)
- Release PR → `main` (after user review/approval)

### Worktree Cleanup Policy

Do NOT clean up worktrees until the user has reviewed them. Cleanup only when explicitly requested.

---

## Agent Team Communication

### Team Configuration

- **Team**: `scmux`
- **team-lead** (you, Claude Code) — manages task list, reviews work, coordinates sprints
- **arch-cmux** is a Codex agent — communicates via ATM CLI messages

### Identity

`.atm.toml` at repo root sets `default_team = "scmux"`.

### Communicating with arch-cmux (Codex)

arch-cmux does **not** monitor Claude Code messages. Use ATM CLI:

```bash
# Send a message
atm send arch-cmux "your message here"

# Check inbox for replies
atm read

# Inbox summary
atm inbox
```

**Nudge arch-cmux** (if no reply after 2 minutes):
```bash
# Find arch-cmux's pane
tmux list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_title} #{pane_current_command}'

# Send nudge
tmux send-keys -t <pane-id> -l "You have unread ATM messages. Run: atm read --team scmux" && sleep 0.5 && tmux send-keys -t <pane-id> Enter
```

### Communication Rules

1. No broadcast messages — all communications are direct
2. Poll for replies — after sending to arch-cmux, wait 30–60s then `atm read`
3. arch-cmux is async — do not block; continue other work and check back

---

## Design Rules (Enforce Always)

1. **Only `scmux-daemon` writes to SQLite** — CLI and web UI are pure HTTP clients
2. **Browser never spawns terminals** — jump always goes through `POST /sessions/:name/jump` → daemon → AppleScript
3. **Unreachable hosts = monochrome + last-known data** — never an error dialog
4. **Missing `gh`/`az` = `tool_unavailable` in `session_ci` table** — rest of system unaffected
5. **No same-poll-cycle retry** on failed session starts

---

## Initialization Process

1. Run: `atm teams resume scmux` (or `TeamCreate` if needed)
2. Run: `atm teams cleanup scmux`
3. Check task list (`TaskList`) for current sprint status
4. Check current branches and worktrees
5. Output concise status summary
6. Identify next sprint ready to execute


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:1105d646 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->
