# scmux Requirements (Planning Baseline)

Status: v0.5.0 planning baseline.
Scope: This document replaces discovery-first assumptions with editor-defined, runtime-read-only behavior.

## 1. Product intent

scmux manages many concurrent AI crews safely.

Primary outcomes:
- Define crews explicitly through editor flows.
- Launch and monitor crews without terminal-window coupling.
- Keep runtime state live and ephemeral.
- Prevent arbitrary runtime code from mutating persistent definitions.

## 2. Canonical language

The UI and API use the design language defined in [design-language.md](./design-language.md).

Hierarchy:
- `Armada`: top-level workspace/view profile.
- `Fleet`: grouping inside an Armada.
- `Flotilla`: optional sub-group inside a Fleet.
- `Crew`: operational team unit.
- `CrewMember`: agent slot in a Crew.
- `CrewVariant`: concrete host/path/branch runtime context.

## 3. Hard invariants

| ID | Requirement |
|---|---|
| INV-01 | SQLite is a definition store only. Runtime discovery/poller data is not persisted as definition truth. |
| INV-02 | Exactly one module performs persistent writes (editor write gate). No other module may write persistent definition data. |
| INV-03 | All pollers (`tmux`, `hosts`, `ci`, `atm`) are persistent-read-only and produce in-memory runtime projection only. |
| INV-04 | Deleting definitions does not reconstruct from tmux discovery. |
| INV-05 | Panic/partial failure must not mass-stop unrelated crews. |
| INV-06 | Crew start is blocked if required configuration cannot resolve (missing prompt files, invalid paths, etc.). |
| INV-07 | Running crews are not force-killed solely due to later config validation failures. |
| INV-08 | Crew rename is disallowed by default; name stays aligned with ATM team naming. |

## 4. Functional requirements

### 4.1 Editing and persistence

| ID | Requirement |
|---|---|
| ED-01 | Provide editor flows for `Armada`, `Fleet`, `Flotilla`, and `Crew` management. |
| ED-02 | Allow add/move operations for `CrewMember` between Fleet/Flotilla organizational contexts. |
| ED-03 | Multiple UI entry points may initiate edits, but all persistent mutations must route through the single writer gate. |
| ED-04 | Editing may use temporary invalid intermediate states in form UI, with clear validation messages. |
| ED-05 | On save, edits must be atomic; if atomicity cannot be guaranteed, save fails and UI refreshes current state. |
| ED-06 | Deleting from Armada/Fleet/Flotilla unlinks references; hard-delete occurs only when reference count reaches zero. |
| ED-07 | Shared-object edit model: edit shared object or explicit copy-on-write fork at edit time. |
| ED-08 | Running Crew allows organization-only edits (Armada/Fleet/Flotilla placement); roster/prompt edits are disallowed while running. |

### 4.2 Crew and CrewMember definition

| ID | Requirement |
|---|---|
| CR-01 | Each Crew must have exactly one `Captain` CrewMember. |
| CR-02 | `Captain` defaults to Claude model selection for ATM compatibility, but any AI/model is allowed. |
| CR-03 | `Mates` and `Bosun` roles are optional and may be multiple. |
| CR-04 | Every CrewMember prompt reference must resolve to concrete text (stored text or resolvable file path). No implicit prompt defaults. |
| CR-05 | A Crew's associated CrewVariant must include concrete host/path/repo context for runtime launch (`host_id`, `root_path`, repo metadata or explicit non-repo root). |
| CR-06 | Crew identity must include a stable cross-host identifier (`crew_ulid`) for future coordination. |

### 4.3 Launch and runtime model

| ID | Requirement |
|---|---|
| RT-01 | Every runnable CrewVariant resolves to concrete tmux coordinates (`session`, `window`, `pane`). |
| RT-02 | `start` creates tmux runtime from definition and launches configured CrewMembers; no iTerm dependency for runtime existence. |
| RT-03 | iTerm/terminal jump is viewer-only attach behavior. |
| RT-04 | Runtime state machine is `stopped -> starting -> running -> idle -> done` (`done` policy may remain non-destructive until finalized). |
| RT-05 | Runtime APIs are served from in-memory projection built from pollers + definitions. |
| RT-06 | Stop behavior for coordinated ATM shutdown is out-of-scope for full implementation in this phase; stub path is acceptable with clear error/log behavior where not implemented. |

### 4.4 Discovery and import

| ID | Requirement |
|---|---|
| DI-01 | Provide a dedicated unregistered-runtime view for raw tmux sessions/windows/panes not currently registered as crews. |
| DI-02 | Discovery view is read-only and never auto-writes to DB. |
| DI-03 | User can explicitly import discovered runtime into a registered Crew definition in an Armada/Fleet context. |
| DI-04 | Import flow must be lightweight and guided for tmux sessions created outside scmux. |

### 4.5 ATM, hosts, and CI

| ID | Requirement |
|---|---|
| AH-01 | ATM integration is pane/member-scoped runtime state, not session-level-only. |
| AH-02 | Permanent ATM roster/team-composition changes require explicit user approval via editor write flow. |
| AH-03 | Host health and CI status are runtime snapshots; they are exposed via API/CLI and not persisted as runtime-definition truth. |
| AH-04 | `scmux doctor` shall expose diagnostic/runtime health information for testability and operations visibility. |

### 4.6 Organization and launch scopes

| ID | Requirement |
|---|---|
| ORG-01 | Launch commands must support `Crew`, `Flotilla`, `Fleet`, and `Armada` scopes. |
| ORG-02 | Armada clone initially references shared underlying Crew/CrewVariant objects (live runtime visible in both views). |
| ORG-03 | Fleet provides visual grouping identity (including color) for related crews. |
| ORG-04 | Flotilla membership is non-overlapping within a Fleet (a Crew belongs to at most one Flotilla in a Fleet). |

### 4.7 Editor API endpoints

| ID | Requirement |
|---|---|
| API-ED-01 | `GET /editor/state` returns Armada/Fleet/Flotilla/Crew/CrewRef data required to render editor state. |
| API-ED-02 | `POST /editor/armadas` creates Armada definitions via the writer gate only. |
| API-ED-03 | `PATCH /editor/armadas/:id` updates Armada definitions via the writer gate only. |
| API-ED-04 | `POST /editor/fleets` creates Fleet definitions via the writer gate only. |
| API-ED-05 | `PATCH /editor/fleets/:id` updates Fleet definitions via the writer gate only. |
| API-ED-06 | `POST /editor/flotillas` creates Flotilla definitions via the writer gate only. |
| API-ED-07 | `PATCH /editor/flotillas/:id` updates Flotilla definitions via the writer gate only. |
| API-ED-08 | `POST /editor/crews` creates Crew bundle definitions (crew, members, variants, placement) atomically via the writer gate only. |
| API-ED-09 | `PATCH /editor/crews/:id` updates Crew bundle definitions atomically via the writer gate only. |
| API-ED-10 | `POST /editor/crews/:id/clone` clones Crew definitions using shared-by-default policy and explicit placement input. |
| API-ED-11 | `POST /editor/crew-refs/:id/move` moves organization placement for an existing Crew reference. |
| API-ED-12 | `DELETE /editor/crew-refs/:id` unlinks Crew reference and performs reference-counted cleanup when the last reference is removed. |

## 5. Non-goals for this planning cycle

- Full ATM shutdown orchestration protocol implementation.
- Full remote-host synchronization design beyond stable identity direction.
- Heavy history/provenance subsystem.
- Automated migration/rename workflow for Crew names.

## 6. Acceptance criteria for planning completion

Planning is complete when:
1. `requirements.md`, `architecture.md`, and `project-plan.md` are mutually consistent.
2. Single-writer gate and poller read-only constraints are explicit and testable.
3. Armada/Fleet/Flotilla/Crew/CrewMember editing flows are defined, including add/move behavior.
4. Unregistered tmux discovery and explicit import flow are defined as view-only + opt-in conversion.
5. Runtime launch model and tmux binding constraints are documented.
