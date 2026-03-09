# Sprint P7-S2 — Runtime-Write Carve-Out and Safety Gate

## Scope

Enforce definition-first persistence boundaries and remove remaining unsafe runtime-write behavior.

## Deliverables

- Remove all persistent writes outside the writer gate.
- Keep runtime pollers projection-only (no definition persistence from runtime observations).
- Ensure API/runtime control paths do not bypass `definition_writer`.
- Stub unsupported ATM shutdown-send behavior with explicit, non-silent behavior when enabled.
- Preserve scoped stop semantics and avoid cross-session side effects.
- Validate carve-out against `docs/refactor-scope-v0.5.md` conflict inventory.

## Requirement IDs

- `INV-01`
- `INV-02`
- `INV-03`
- `INV-04`
- `INV-05`
- `ED-03`
- `RT-05`
- `RT-06`

## Acceptance Criteria

1. Code search confirms no persistent-definition writes outside writer-gate pathways.
2. Poller/runtime modules remain SQLite-read-only for definitions.
3. `atm::send_shutdown_messages` returns early when `allow_shutdown = false`.
4. Tests cover:
- ATM shutdown gate behavior (`allow_shutdown` false).
- ATM team source remains config-driven (no `~/.claude/teams` scan behavior).
5. Existing runtime flows and test suite remain green after carve-out changes.
