# scmux Project Plan (Execution Reset)

Status: Active v0.5.0 planning baseline.

## 1. Objective

Bring scmux to a stable MVP where users can reliably define, launch, and manage many AI crews with strict persistence controls.

Top priorities:
- Documentation and design language locked first.
- Remove unsafe/legacy runtime-write behavior.
- Implement editor-first definitions for Armada/Fleet/Flotilla/Crew/CrewMember.
- Keep runtime state live and non-persistent.

## 1.1 Phase map

| Phase | Theme | Target Branch |
|---|---|---|
| 1-6 | Historical delivery (foundation through architecture realignment) | `integrate/phase-1` ... `integrate/phase-6` |
| 7 | Crew hierarchy, editor backend/UI, launch/import hardening | `integrate/phase-7` |

## 2. Scope boundaries

In scope for this plan:
- `docs/requirements.md`, `docs/architecture.md`, `docs/project-plan.md` alignment.
- Single persistent write gate implementation direction.
- Editor flows above Crew editor (Armada/Fleet/Flotilla input/edit + CrewMember add/move).
- Unregistered tmux discovery as view + explicit import.
- tmux-bound launch/runtime model.

Out of scope for this cycle:
- Full ATM shutdown orchestration implementation.
- Heavy history/provenance subsystem.
- Broad remote multi-daemon sync engine beyond stable identity direction.
- Deferred issue set tracked for this cycle: GH `#31`, `#35`, `#36`, `#37`.

## 3. Sprint roadmap

### Sprint P7-S1: Planning lock (docs + decision closure)

Deliverables:
- Finalize `requirements.md`, `architecture.md`, `project-plan.md`.
- Align terminology with `design-language.md`.
- Record unresolved policy decisions explicitly.

Exit criteria:
- Docs are mutually consistent.
- Team can execute implementation without redefining core behavior.

### Sprint P7-S2: Runtime-write carve-out and safety gate

Deliverables:
- Remove all persistent writes outside the writer gate.
- Enforce visibility boundary so only writer module can mutate definitions.
- Stub unsupported ATM send/shutdown paths with explicit error logs (no silent behavior).
- Keep pollers read-only and projection-only.
- Execute carve-out against explicit conflict inventory in [docs/refactor-scope-v0.5.md](./refactor-scope-v0.5.md).

Exit criteria:
- Grep/code review confirms no stray definition writes outside writer gate.
- Existing runtime flows continue without DB reconstruction behavior.

### Sprint P7-S3: Definition schema and editor backend

Deliverables:
- Persisted model for Armada/Fleet/Flotilla/Crew/CrewMember/CrewVariant.
- API endpoints for create/edit/clone/move/unlink across hierarchy.
- Atomic save operations and reference-counted delete semantics.
- Running-edit restrictions enforced (org moves allowed; roster/prompt edits blocked while running).

Exit criteria:
- Editor API coverage proves atomic updates and policy enforcement.
- Copy-on-write fork path is explicit and testable.

### Sprint P7-S4: UI editors and organization workflows

Deliverables:
- Armada/Fleet editor screens, plus Flotilla editor screens when Flotilla is enabled for v1 (otherwise behind feature flag).
- Basic edit controls above Crew editor (add/move crews and crew members as defined by policy).
- Clone flows for Armada/Fleet/Crew with shared-by-default semantics.
- Clear validation UX for unresolved references.

Exit criteria:
- User can create full hierarchy and reorganize without direct DB manipulation.
- Invalid definitions can be repaired via UI without dead-end validation traps.

### Sprint P7-S5: Launch/runtime and discovery import

Deliverables:
- Strict start validation and tmux binding from CrewVariant.
- Runtime projection endpoints for dashboard/CLI.
- Unregistered discovery view (sessions/windows/panes) and explicit import flow.
- `scmux doctor` diagnostics surface for runtime visibility.

Exit criteria:
- External tmux sessions can be imported into registered crews.
- Launch/monitor workflows work without runtime persistence side effects.

### Sprint P7-S6: Hardening and release prep

Deliverables:
- Concurrency/race hardening around edit/save and runtime transitions.
- Safety testing for panic/partial failure isolation.
- Documentation final pass and release checklist updates.

Exit criteria:
- QA critical findings closed.
- MVP acceptance checklist passes for create/edit/launch/manage/import flows.

## 4. Policy decisions to finalize during execution

1. Final persisted representation of Armada/Fleet/Flotilla (entity vs view-layer only where applicable).
2. Precise clone matrix for “shared edit” vs “fork” by field category.
3. Exact uniqueness constraints across `crew_ulid`, host, repo, branch, and path dimensions.
4. Default UI organization mode and cross-host merge behavior.
5. `crew_ulid` lifecycle ownership and conflict resolution.
6. Import mapping rules from discovered tmux objects to Crew/CrewMember schema.
7. Flotilla rollout mode (required in v1 vs feature flag).

## 5. MVP acceptance checklist

MVP is considered successful when all are true:
1. User can define Armada/Fleet/Flotilla/Crew/CrewMembers entirely from editors.
2. User can add/move crew members and crews across organization layers per policy.
3. Start launches tmux runtime from definitions with strict validation.
4. Runtime state is visible and testable without writing runtime snapshots to DB.
5. Unregistered tmux runtime can be discovered and imported explicitly.
6. No persistent writes exist outside writer gate.
7. Panic/error paths do not mass-stop unrelated crews.

## 6. Coordination and review cadence

- Team-lead owns sprint assignment/acceptance and policy decisions.
- Implementation reports include: changed files, invariant checks, tests run, unresolved blockers.
- QA review runs after each sprint, with blocking findings fed back before next sprint starts.
