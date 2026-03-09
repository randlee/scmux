# scmux Architecture (Target)

Status: v0.5.0 planning baseline architecture.

## 1. Architecture summary

scmux uses a strict split:
- Persistent definition plane: editor-driven only.
- Runtime observation plane: pollers + in-memory projection.

This split exists to prevent accidental runtime writes and to keep launch/control robust for many concurrent AI sessions.

## 2. System planes

### 2.1 Definition plane (persistent)

Owned by one writer module only.

Responsibilities:
- Persist Armada/Fleet/Flotilla/Crew/CrewMember definitions.
- Enforce write constraints and identity rules.
- Validate save operations atomically.

### 2.2 Runtime plane (ephemeral)

Owned by read-only poller modules and projector.

Responsibilities:
- Observe tmux runtime, host health, CI status, ATM member state.
- Build in-memory session/crew projection.
- Serve runtime APIs and UI cards without mutating persistent definitions.

## 3. Hard boundaries

1. Exactly one persistent writer module.
- Proposed module: `definition_writer` (name can be finalized during implementation).
- All persistence mutating handlers call this module only.

2. Pollers are write-prohibited for definitions.
- `tmux_poller`, `hosts`, `ci`, `atm` must not persist discovery as definitions.

3. Discovery is import-only.
- Unregistered tmux discovery view does not write DB.
- Explicit user import invokes `definition_writer`.

4. Runtime failures are scoped.
- Panic/error in one crew flow cannot stop unrelated running crews.

## 4. Module responsibilities

### 4.1 `definition_writer`

Only persistent writer.

Responsibilities:
- CRUD for Armada/Fleet/Flotilla/Crew/CrewMember.
- Reference/link updates for organization membership and move operations.
- Copy-on-write fork operations when user chooses split.
- Atomic transaction boundaries for complex edits.
- Validation at save/start boundaries.

### 4.2 `tmux_poller` (current `scheduler.rs` rename target)

Responsibilities:
- Read tmux sessions/windows/panes.
- Resolve runtime bind for CrewVariants.
- Feed projector with runtime session state.

Prohibited:
- Writing definitions or reconstruction data to SQLite.

### 4.3 `hosts.rs`

Responsibilities:
- Read remote daemon reachability/health snapshots.
- Feed host runtime status into projector.

Prohibited:
- Persistent definition mutation from runtime polling.

### 4.4 `ci.rs`

Responsibilities:
- Read CI state snapshots (provider adapters).
- Feed per-crew/per-project status into projector.

Prohibited:
- Persisting CI runtime snapshots as definition truth.

### 4.5 `atm.rs`

Responsibilities:
- Read ATM daemon runtime member status per crew member.
- Map runtime status by configured identity references.

Prohibited:
- Autonomous roster/team persistent writes.

### 4.6 `api.rs`

Responsibilities:
- Read endpoints: definitions + runtime projection responses.
- Runtime actions: start/stop/jump pathways.
- Editor endpoints: call `definition_writer` for persistence.

Constraint:
- No direct persistence calls from non-editor routes.

## 5. Core data model

Logical hierarchy:
- Armada
- Fleet
- Flotilla (optional)
- Crew
- CrewMember
- CrewVariant

Hierarchy model:

```text
Armada
  -> Fleet
    -> Flotilla (optional)
      -> Crew
        -> CrewMember (role/model/prompt definition)
        -> CrewVariant (host/path/branch runtime binding)
```

Key identity notes:
- Crew has immutable-by-default `crew_name` aligned with ATM naming.
- Crew has stable `crew_ulid` for cross-host identity continuity.
- CrewVariant binds concrete runtime context (`host_id`, repo metadata, branch/path dimensions, tmux coordinates).

## 6. Runtime binding model

Every runnable CrewVariant must resolve to tmux coordinates:
- `session`
- `window`
- `pane`

Implications:
- Armada/Fleet/Flotilla are organization/view layers.
- Cloning Armada does not duplicate live runtime by default; shared references show same running sessions until explicit fork.

## 7. Editing and validation model

### 7.1 Save-time behavior

- Forms may allow temporary invalid states while editing.
- Save must be atomic.
- If save cannot be applied atomically, it fails and client refreshes current DB state.

### 7.2 Start-time behavior

- Start gate is strict: unresolved required references (e.g., missing prompt files) block start.
- Already running crews are not force-killed solely because a later validation check fails.

### 7.3 Running edit restrictions

Allowed while running:
- move Crew between Armada/Fleet/Flotilla
- organizational/view edits

Disallowed while running:
- Crew roster/prompt definition edits

## 8. Discovery/import architecture

- `GET /discovery` (or equivalent read endpoint) exposes unregistered tmux runtime objects.
- Discovery view is non-persistent.
- Import action maps selected discovered runtime to Crew/CrewMember definition payload and persists via `definition_writer`.

## 9. Launch and stop architecture

### 9.1 Launch

1. API resolves Crew/CrewVariant definition.
2. Validate start prerequisites.
3. Create/attach tmux session layout.
4. Launch CrewMember commands by pane.
5. Runtime projector reflects transitions (`stopped -> starting -> running/idle`).

### 9.2 Stop

- Full coordinated ATM shutdown orchestration is intentionally deferred.
- Current phase allows stub behavior with explicit log/error reporting when shutdown command path is not implemented.
- Any hard-stop fallback must stay scoped to the targeted crew runtime.

## 10. Observability direction

- Structured logs with consistent action/session/member identifiers.
- Keep logging schema OpenTelemetry-ready for near-term integration.
- `scmux doctor` is the primary operational surface for runtime diagnostics.

## 11. Prohibited behaviors

- Auto-registration of discovered tmux runtime into DB.
- Poller-driven persistent definition writes.
- Reconstruction of deleted definitions from live runtime.
- Session-wide-only ATM status as final model (member-level is required).
- Unscoped mass shutdown on panic/error.
