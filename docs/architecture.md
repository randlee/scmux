# scmux — Architecture

## 1. Product Architecture (Target)

scmux is a multi-team operations dashboard and launcher for AI agent teams.

Primary value: show all defined projects at once (running or stopped), expose per-agent runtime state, show CI health, and start/stop safely without requiring a terminal window.

## 2. Hard Invariants

1. SQLite is a definition store only.
- Stores user-approved project/host/roster definitions.
- Does not auto-ingest tmux discovery.

2. Exactly one persistent writer subsystem exists.
- Multiple editor entry points are allowed (`New Project`, `Project Editor`, card-level `Edit`), but all persistent writes must route through the same writer subsystem.
- All pollers and runtime loops are SQLite read-only.

3. Approved-project write policy is mandatory.
- Any persistent write must validate that target project is approved.

4. Discovery is read-only.
- Raw tmux discovery is informational and never mutates project definitions.

5. Safety-first session control.
- Stop is graceful-first (ATM shutdown request), then scoped hard-stop only if needed.
- Panic/error paths must not bulk-stop unrelated sessions.

6. Runtime state is live/ephemeral.
- Runtime status is refreshed continuously for UI/CLI display.
- Runtime status is not persisted as definition truth.

## 3. Runtime Data Flow

```text
User Form/Edit Action
  -> Definition Write Module (only persistent writer)
  -> SQLite definitions (projects/hosts/approved roster)

Pollers (read-only persistence)
  -> tmux_poller reads tmux runtime
  -> atm_poller reads ATM socket runtime
  -> ci_poller reads gh/az runtime
  -> hosts_poller reads remote health/runtime

Runtime projection
  -> in-memory aggregate session/project view
  -> HTTP API
  -> Dashboard + CLI
```

## 4. Module Responsibilities

### 4.1 `definition_writer` (new/central)
- Only module allowed to persist project definitions.
- Can be invoked by multiple editor UX entry points.
- Handles create/edit/delete for projects/hosts/approved roster changes.
- Enforces approved-project constraints.

### 4.2 `tmux_poller` (rename target for current `scheduler.rs`)
- Reads tmux session/pane runtime state.
- Computes runtime state (`stopped|starting|running|idle|done`) for API projection.
- No persistent writes.

### 4.3 `hosts.rs`
- Reads remote daemon health/runtime snapshots.
- Aggregates reachability and stale status for dashboard.
- No persistent writes from discovery.

### 4.4 `ci.rs`
- Reads CI status from `gh`/`az`.
- Produces runtime snapshot list per project (PRs + run statuses).
- No persistent writes from CI polling.

### 4.5 `atm.rs`
- Reads ATM socket runtime state per agent.
- Maps per-pane state via `config_json` (`atm_agent`, `atm_team`).
- Permanent roster changes are editor-driven writes only.

### 4.6 `api.rs`
- Read endpoints expose aggregated runtime + persisted definitions.
- Action endpoints (`start/stop/jump`) control runtime only.
- Persistent writes are only via definition-editor endpoints.

## 5. Session Lifecycle Model

State machine:

`stopped -> starting -> running -> idle -> done`

- `stopped`: project exists in definitions; runtime session absent.
- `starting`: launch requested; tmux/agents being created.
- `running`: tmux exists and at least one pane agent is ATM-active.
- `idle`: tmux exists; all pane agents idle/offline.
- `done`: semantics and auto-teardown policy are intentionally unresolved and remain non-destructive by default.

## 6. Key Flows

### 6.1 Start
`POST /sessions/:name/start`
1. Read `config_json` definition from SQLite.
2. Create tmux session/window/pane layout.
3. Launch each pane command.
4. Publish runtime transitions to API/dashboard.

### 6.2 Stop
`POST /sessions/:name/stop`
1. Send ATM shutdown signal/message to configured agents.
2. Wait configurable grace period.
3. If still running, apply scoped hard-stop to target session only (exact retries/timeouts are product-configurable and pending finalization).
4. Never kill unrelated sessions.

### 6.3 Jump
`POST /sessions/:name/jump`
- Opens iTerm (or selected terminal) as a viewer.
- Attach/detach does not control lifecycle.

## 7. Views

### 7.1 Primary View
- All defined projects from SQLite.
- Includes stopped projects with Start affordance.
- Running projects show per-pane ATM + CI runtime status.

### 7.2 Secondary View
- Raw tmux discovery tab.
- Informational only, no persistence side effects.

## 8. Definition Schema (`config_json`)

```json
{
  "panes": [
    {
      "name": "team-lead",
      "command": "claude --profile team-lead",
      "atm_agent": "team-lead",
      "atm_team": "scmux-dev"
    },
    {
      "name": "arch-cmux",
      "command": "codex --profile arch-cmux",
      "atm_agent": "arch-cmux",
      "atm_team": "scmux-dev"
    }
  ],
  "repo": "/Users/randlee/Documents/github/scmux",
  "window_layout": "even-horizontal",
  "atm_team": "scmux-dev"
}
```

## 9. Observability Direction

Current logging should remain structured and stable with OpenTelemetry in mind.

Required now:
- Correlation identifiers on lifecycle/action events.
- Consistent key/value fields for session/project/agent/host.
- Log/event schema that can be mapped to OTel traces/spans without redesign.

## 10. Prohibited Behaviors

- Auto-writing project definitions from tmux discovery.
- Reconstructing deleted definitions from live tmux.
- Poller-based persistent writes (tmux/hosts/ci/atm loops).
- Session-level-only ATM model as final representation.
- Panic/error bulk-stop behavior.

## 11. Refactor Scope (Code Conflicts to Remove)

Conflicting current modules and expected direction:

- `scheduler.rs`
  - Remove discovery-to-definition persistence and reconstruction logic.
  - Rename responsibility toward read-only runtime polling (`tmux_poller`).

- `hosts.rs`
  - Remove remote discovery persistence paths.
  - Keep health/runtime aggregation only.

- `ci.rs`
  - Remove CI snapshot persistence writes.
  - Keep runtime CI projection only.

- `atm.rs`
  - Remove autonomous persistent runtime writes.
  - Keep per-pane runtime mapping; route roster persistence through editor.

- `api.rs`
  - Ensure only definition-editor routes can persist data.
  - Keep start/stop/jump as runtime control paths.

This architecture supersedes earlier discovery-first assumptions.
