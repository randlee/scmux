---
name: codex-orchestration
description: Orchestrate multi-sprint phases where arch-ctm (Codex) is the sole developer, with pipelined QA via quality-mgr teammate. Team-lead tracks findings and schedules fix passes.
---

# Codex Orchestration

This skill defines how the team-lead (ARCH-ATM) orchestrates phases where **arch-ctm (Codex)** is the sole developer, executing sprints sequentially while QA runs in parallel via a dedicated **quality-mgr** teammate.

**Audience**: Team-lead only.

**When to use**: When a phase's implementation is done entirely by arch-ctm (a Codex agent communicating via ATM CLI), not by Claude Code scrum-masters. This pattern was proven in Phase M (8 sprints) and Phase O.

## Prerequisites

Before starting a phase:
1. Phase plan document exists with sprint specs and dependencies
2. Integration branch `integrate/phase-{P}` created off `develop`
3. ATM team (`atm-dev`) is active with team-lead and arch-ctm as members
4. arch-ctm is running and reachable via ATM CLI (`atm send arch-ctm "ping"`)

## Architecture

```
team-lead (ARCH-ATM)
  ├── arch-ctm (Codex) ──── sole developer, sequential sprints
  │     communicates via ATM CLI only
  └── quality-mgr (Claude Code) ──── QA coordinator teammate
        spawns rust-qa-agent + scmux-qa-agent as background agents
```

Key principle: **arch-ctm does NOT wait for QA**. He proceeds to the next sprint as soon as he completes one, unless there are outstanding fix requests from earlier sprints.

## Phase Setup

### 1. Create Integration Branch

```bash
git fetch origin develop
git branch integrate/phase-{P} origin/develop
git push -u origin integrate/phase-{P}
```

### 2. Create First Sprint Worktree

```bash
# Use sc-git-worktree skill
/sc-git-worktree --create feature/p{P}-s1-{slug} integrate/phase-{P}
```

### 3. Spawn Quality Manager

Spawn once per phase. The quality-mgr persists across all sprints.

Use the Task tool with `name` parameter to spawn as a tmux teammate:

```json
{
  "subagent_type": "general-purpose",
  "name": "quality-mgr",
  "team_name": "atm-dev",
  "model": "sonnet",
  "prompt": "You are quality-mgr for Phase {P}. You will receive QA assignments from team-lead for each sprint as they complete. Stand by for first assignment. Integration branch: integrate/phase-{P}. Phase docs: docs/project-plan.md, docs/requirements.md, docs/architecture.md."
}
```

**Tmux teammate launch troubleshooting**: If the pane opens but the Claude process doesn't start, manually launch in the pane with all three required flags:

```bash
CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 /Users/randlee/.local/share/claude/versions/<VERSION> \
  --agent-id quality-mgr@atm-dev \
  --agent-name quality-mgr \
  --team-name atm-dev
```

All three flags (`--agent-id`, `--agent-name`, `--team-name`) are required together — omitting any one causes an error.

### 4. Send O.1 Assignment to arch-ctm

```bash
atm send arch-ctm "Phase {P} Sprint {P}.1 assignment: {title}

Worktree: /path/to/worktree
Branch: feature/p{P}-s1-{slug}
PR target: integrate/phase-{P}

Deliverables:
- {list deliverables}

Requirements: docs/requirements.md ({relevant FRs})
Sprint plan: docs/project-plan.md (Phase {P} section)

When complete: commit, push, create PR targeting integrate/phase-{P}, then notify me via atm send."
```

## Sprint Pipeline

### Steady-State Flow

```
arch-cmux:    implement → commit/push → notify → cargo test → idle (next task already in inbox)
team-lead:                             ↓ on push notification (NOT after cargo test):
                              create PR + next worktree + send S+1 assignment
quality-mgr:                           [── QA S ──────────────────────]
```

**Key principle: team-lead acts on push, not on test completion.** By the time arch-cmux finishes cargo test, the next assignment must already be in his inbox and the next worktree ready.

### When arch-cmux Pushes Sprint S

On receiving arch-cmux's push notification (commit hash + branch), do ALL of the following immediately in parallel — do NOT wait for cargo test results:

1. **Open PR** for sprint S targeting `integrate/phase-{P}`:
   ```bash
   gh pr create --title "..." --base integrate/phase-{P} --head feature/p{P}-s{N}-{slug}
   ```
2. **Create worktree for S+1** based on sprint S branch:
   ```
   /sc-git-worktree --create feature/p{P}-s{N+1}-{slug} feature/p{P}-s{N}-{slug}
   ```
   All worktrees chain: S+1 bases on S, so later sprints include earlier work.
3. **Assign QA to quality-mgr** via SendMessage using rendered `qa-template.xml.j2`.
4. **Check for outstanding findings** from earlier sprints:
   - If findings exist for S-2 or S-1: send fix assignment to arch-cmux BEFORE S+1 assignment
   - If no findings: send S+1 assignment immediately using rendered `dev-template.xml.j2`
5. **arch-cmux addresses fixes first, then starts S+1**

### When arch-ctm Has Outstanding Findings

Priority order for arch-ctm:
1. Fix findings on oldest sprint first (S-2 before S-1)
2. Merge fixes forward into later sprint worktrees
3. Then proceed to next sprint

Fix workflow:
```bash
# arch-ctm fixes on the sprint's original worktree
# arch-ctm pushes fix commits to same PR branch
# team-lead asks quality-mgr to re-run QA on the fixed worktree
# If QA passes, team-lead merges PR to integration branch
```

### Merge Forward Protocol

After fixes merge to `integrate/phase-{P}`:
- arch-ctm must merge integration branch into any active sprint worktree before continuing:
  ```bash
  git fetch origin
  git merge origin/integrate/phase-{P}
  ```
- This ensures later sprints include all fixes from earlier sprints

## QA Coordination

### Team-lead → quality-mgr Messages

Assignment format:
```
Run QA on Sprint {P}.{S}: {title}
Worktree: {absolute path}
Sprint deliverables: {bullet list}
Design docs: {list of relevant doc paths}
PR: #{number}
```

Re-run after fixes:
```
Re-run QA on Sprint {P}.{S} (post-fix).
Worktree: {path}
Fixed findings: {list of QA IDs addressed}
```

### quality-mgr → team-lead Reports

quality-mgr reports PASS/FAIL with finding IDs. Team-lead tracks:

| Sprint | QA Run | Verdict | Blocking Findings | Status |
|--------|--------|---------|-------------------|--------|
| O.1    | 1      | FAIL    | QA-001, QA-002    | Fixes assigned |
| O.1    | 2      | PASS    | —                 | Merged |
| O.2    | 1      | PASS    | —                 | Merged |

### Finding Lifecycle

```
OPEN → assigned to arch-ctm → FIXED (arch-ctm pushes) → re-QA → VERIFIED (QA passes)
                             → WONTFIX (team-lead approves deviation)
```

## PR and Merge Strategy

- **All PRs target `integrate/phase-{P}`** (never develop directly)
- **Merge order**: Sprint PRs merge in order (S.1 before S.2)
- **Merge gate**: QA pass + CI green
- **Team-lead merges** (not arch-ctm)
- After all sprints merge: one final PR `integrate/phase-{P} → develop`

## ATM Communication Protocol

All arch-ctm communication is via ATM CLI. Follow the dogfooding protocol (ACK → work → complete → ACK).

### Sending assignments
```bash
atm send arch-ctm "message"
```

### Checking for replies
```bash
atm read
```

### When arch-cmux goes idle

Do NOT nudge via tmux send-keys — this interrupts active work. Instead, ensure the next task is already in his ATM inbox before he finishes the current sprint. If the inbox is populated and he goes idle, ATM delivery will surface the message when he polls. No intervention needed.

## Phase Completion

After all sprints pass QA and merge to integration branch:
1. Run final integration QA (quality-mgr validates full integration branch)
2. Create PR: `integrate/phase-{P} → develop`
3. Wait for CI green
4. Merge after user approval
5. Shutdown quality-mgr teammate
6. Do NOT clean up worktrees until user reviews

## Anti-Patterns

- Do NOT tell arch-ctm to wait for QA before starting the next sprint
- Do NOT skip QA on any sprint — quality-mgr runs both agents every time
- Do NOT merge PRs without QA pass + CI green
- Do NOT let findings accumulate — schedule fixes before assigning new sprints
- Do NOT create worktrees off `develop` — chain from previous sprint or integration branch
- Do NOT communicate with arch-ctm via SendMessage — use ATM CLI only
- Do NOT reuse quality-mgr across phases — spawn fresh per phase
- Do NOT clean up worktrees without user approval
