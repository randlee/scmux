# Sprint 4.1 — Supervision + Performance

- Sprint ID: `4.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s1-supervision`
- Base branch: `integrate/phase-4`
- PR target: `integrate/phase-4`

## Context

Functional scope is complete; operational hardening and service management are required for production reliability.

## Deliverables

1. Service assets
- `deploy/macos/com.scmux.scmux-daemon.plist` (new)
- `deploy/linux/scmux-daemon.service` (new)
- install instructions in docs.

2. Daemon status and health improvements
- finalize `scmux daemon status` behavior against health endpoint.
- validate retention/pruning visibility.

3. Performance + resilience pass
- benchmark poll cycle latency (<500ms at 50 sessions).
- benchmark read endpoint latency (<100ms).
- harden error isolation for single host/session failures.

4. Test additions
- targeted integration/perf checks for NF and DH acceptance conditions.

## Acceptance Criteria

- launchd and systemd assets are functional and documented.
- NF-03/NF-04 thresholds are measured and satisfied.
- daemon remains stable under isolated host/session failures.

## Requirement IDs Covered

- `DH-04`, `DH-05`
- `NF-01`, `NF-02`, `NF-03`, `NF-04`, `NF-05`, `NF-08`

## Dependencies

- Requires Sprint `3.2` merged.
- Must merge before Sprint `4.2`.
