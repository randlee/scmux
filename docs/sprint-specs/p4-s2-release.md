# Sprint 4.2 — E2E Tests + Release

- Sprint ID: `4.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s2-release`
- Base branch: `feature/p4-s1-supervision`
- PR target: `integrate/phase-4`

## Context

Final sprint validates complete system behavior and prepares release artifacts.

## Deliverables

1. Scheduler clock abstraction
- Introduce injectable time source (`Clock` trait or equivalent) used by scheduler poll-cycle timing decisions.
- Use injected clock in automated E2E cron test (`T-E-07`).

2. End-to-end tests (automated)
- Add `tests/e2e_tests.rs` equivalent suites for:
  - `T-E-01..T-E-05`
  - `T-E-07`
  - `T-E-10`
  - `T-E-11`
- Use fake tmux binaries (`SCMUX_TMUX_BIN`/`SCMUX_TMUXP_BIN`) where needed.

3. End-to-end manual runbooks
- `docs/e2e-manual-runbook.md` covering:
  - `T-E-06` (iTerm2 jump)
  - `T-E-08` (VPN disconnect → monochrome)
  - `T-E-09` (VPN reconnect → restore)
- `docs/e2e-environment.md` with explicit prerequisites.

4. Release and acceptance docs
- `docs/release-checklist.md` with `AC-01..AC-10` pass/fail state.
- `AC-02` recorded as manual attestation (not a CI gate).
- `docs/release-notes-v0.3.0.md` draft.
- Homebrew formula update noted as placeholder checklist item (non-blocking).

5. Release pipeline artifacts
- Bump workspace version to `0.3.0`.
- Add perf-gate CI execution for `T-D-22`/`T-D-23` in `--release`.

## Acceptance Criteria

- Automated E2E (`T-E-01..05`, `T-E-07`, `T-E-10`, `T-E-11`) passes.
- Manual runbooks for `T-E-06`, `T-E-08`, `T-E-09` are complete and actionable.
- Requirements section 7 acceptance criteria are tracked in release checklist.
- Release checklist and notes are complete and reviewable.

## Requirement IDs Covered

- `T-E-01..T-E-11` (with `T-E-06/08/09` manual)
- Section 7 acceptance criteria (AC-01..AC-10)

## Dependencies

- Requires Sprint `4.1` merged.
- Final pre-release sprint.
