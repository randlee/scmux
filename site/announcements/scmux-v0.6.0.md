# scmux v0.6.0 — Armada/Fleet/Flotilla/Crew Architecture

**Released:** March 10, 2026 · **Install:** binary archives from [releases](https://github.com/randlee/scmux/releases) (macOS arm64, Linux x86_64, Windows x64); Homebrew `randlee/tap` and crates.io `scmux`/`scmux-daemon` publish 0.5.0 — the 0.6.0 publication is pending

[Release notes](https://github.com/randlee/scmux/releases/tag/v0.6.0) · [Architecture](https://github.com/randlee/scmux/blob/main/docs/architecture.md)

---

## Fleet Operator (Agent Orchestrator)

**As a fleet operator, I want declarative tmux session configs and a cross-host dashboard, so that I can orchestrate every agent session from one seat.**

v0.6.0 replaces scmux's flat session list with a persisted fleet hierarchy: **Armada → Fleet → Flotilla (optional) → Crew → CrewMember / CrewVariant**. A CrewMember carries the role/model/prompt definition for an agent, while a CrewVariant binds that definition to a concrete runtime — host, repo metadata, branch/path, and tmux coordinates. Armada, Fleet, and Flotilla are organization and view layers, so you can model a whole multi-host agent organization instead of a bag of windows.

The editor backend lands create/edit/clone/move/unlink behind a WriteGuard compile-boundary, and the dashboard ships Armada/Fleet/Crew editor screens with clone affordances. Cloning an Armada shares references to the same live runtime by default — no duplicate sessions until you explicitly fork — so restructuring your org doesn't spin up a second copy of every agent.

---

## Agent Ops Engineer

**As an agent-ops engineer, I want observability and management over agent sessions, so that I can see what each agent is doing and intervene when needed.**

`scmux doctor` now probes the ATM socket and reports runtime crew diagnostics, so a fleet's messaging backbone and its session graph are checked in one pass. `GET /runtime/crews` projects live crew state, and `GET /runtime/discovery/unregistered` lists tmux sessions that exist outside your definitions — the raw material for pulling stray sessions back under management.

Concurrency hardening closes the gaps that made concurrent edits unsafe: a SESSION_ACTIONS lease serializes session lifecycle, roster-edit guards block conflicting writes, and a TOCTOU fix removes the check-then-act race. Phase 7 shipped at 101 tests passing with zero clippy warnings.

---

## Agent Scheduler

**As a scheduler, I want agent sessions to start on a schedule, so that recurring agent work runs without me manually launching windows.**

Crew variants now start under strict validation: `root_path` and runtime `binding` are checked before a session launches, so a misconfigured schedule fails loudly at definition time instead of spawning a broken window later. `POST /editor/import-discovery` folds discovered tmux sessions into crew bundles, giving scheduled work a clean path from ad-hoc session to managed, restartable crew member.

---

## What's Next

With the Armada/Fleet/Flotilla/Crew model landed, subsequent work completes the 0.6.0 crate and Homebrew publication and extends the definition-driven organization surface.
