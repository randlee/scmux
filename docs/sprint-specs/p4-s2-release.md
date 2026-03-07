# Sprint 4.2 — E2E Tests + Release

- Sprint ID: `4.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s2-release`
- Base branch: `feature/p4-s1-supervision`
- PR target: `integrate/phase-4`

## Context

Final sprint validates complete system behavior and prepares release artifacts.

## Deliverables

1. Clock abstraction for scheduler (prerequisite for cron E2E)
- Add `Clock` trait (or `now: fn() -> DateTime<Utc>` parameter) to `AppState`/scheduler so tests can inject deterministic time.
- Required for T-E-07 (`scmux list` cron session started at scheduled time) to be automatable without wall-clock sleeping.
- If Clock injection proves too invasive, T-E-07 may be demoted to manual test and this deliverable omitted — confirm with team-lead before omitting.

2. End-to-end tests
- `tests/e2e_tests.rs` — automated suite covering **T-E-01..T-E-05, T-E-07, T-E-10, T-E-11** only.
- Perf benchmarks (T-D-22/T-D-23) run via `cargo test --release` with `SCMUX_TMUX_BIN` fake tmux binary; separated into a `perf-gate` job to avoid polluting functional test output.
- `docs/e2e-manual-runbook.md` — manual test runbooks for:
  - **T-E-06**: iTerm2 jump (requires iTerm2 + display; not automatable headless)
  - **T-E-08**: VPN disconnect → monochrome (requires two machines + VPN)
  - **T-E-09**: VPN reconnect → restore (same)
- `docs/e2e-environment.md` — E2E environment prerequisites:
  - macOS 14+ (Sonoma) primary machine
  - tmux ≥ 3.3, tmuxp ≥ 1.30
  - iTerm2 ≥ 3.5 (for T-E-06 manual test)
  - `gh` CLI authenticated (for CI badge tests)
  - Single machine sufficient for automated suite; second machine required for T-E-08/09

2. Acceptance verification report
- `docs/release-checklist.md` — AC-01..AC-10 checklist with pass/fail column.
- **AC-02 (24h soak)**: manual attestation — run daemon on dev machine for 24h, record result in release checklist. Not a CI gate.
- Release checklist must be complete before v1.0.0 tag.

3. Release pipeline artifacts
- Version update to `1.0.0` in workspace `Cargo.toml`.
- `docs/release-notes-v1.0.0.md` — must include: what changed since v0.x, known limitations, install instructions (macOS launchd + Linux systemd), upgrade path (none for first release).
- Homebrew formula update checklist (note: Homebrew tap not yet created; checklist is a placeholder for post-release packaging).

## Acceptance Criteria

- Automated E2E suite (T-E-01..T-E-05, T-E-07, T-E-10, T-E-11) passes in CI.
- Manual runbooks present and complete for T-E-06, T-E-08, T-E-09.
- AC-02 (24h soak) attested manually in release checklist.
- Release checklist AC-01..AC-10 complete and reviewable.
- `cargo clippy` clean, `cargo test --workspace` passes.

## Requirement IDs Covered

- `T-E-01..T-E-11`
- Section 7 acceptance criteria (AC-01..AC-10)

## Dependencies

- Requires Sprint `4.1` merged.
- Final pre-release sprint.
