# Sprint 3.2 — CLI Binary

- Sprint ID: `3.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p3-s2-cli`
- Base branch: `feature/p3-s1-ci`
- PR target: `integrate/phase-3`

## Context

CLI crate exists as a placeholder and does not yet communicate with daemon APIs.

## Deliverables

1. `crates/scmux/Cargo.toml`
- Add runtime deps:
  - `clap`
  - `reqwest`
  - `tokio`

2. `crates/scmux/src/main.rs`
- Implement command surface:
  - `list`, `show`, `start`, `stop`, `jump`, `add`, `edit`, `disable`, `enable`, `remove`, `hosts`, `daemon status`.

3. `crates/scmux/src/client.rs` (new)
- Add HTTP client wrapper with:
  - host resolution from `SCMUX_HOST` or `--host`
  - typed request helpers per daemon route.

4. `crates/scmux/src/output.rs` (optional new helper)
- Consistent terminal output formatting for success/errors.

5. `tests/cli_tests.rs` (new)
- Basic command parsing and URL resolution tests.

## Acceptance Criteria

- CLI command set matches requirements section 4.12.
- All commands call daemon API endpoints correctly.
- `scmux list` and `scmux show` match daemon data contracts.
- `SCMUX_HOST` and `--host` overrides function correctly.

## Requirement IDs Covered

- `CLI-01..CLI-14`
- `API-04..API-16` (as consumed by CLI)

## Dependencies

- Requires Sprint `3.1` merged.
- Must merge before Phase 4 starts.
