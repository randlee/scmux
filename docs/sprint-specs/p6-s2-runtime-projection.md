# Sprint 6.2 — Runtime Projection Dashboard Wiring

## Summary

Sprint 6.2 wires the dashboard to the Phase 6 runtime-projection model:
- primary view from persisted definitions (`GET /sessions`)
- secondary discovery view from raw tmux data (`GET /discovery`)
- runtime controls (`start`/`stop`) and editor entry points (`new`/`edit`) against writer-gated APIs

## Scope

- Dashboard view model updates for runtime projection responses.
- Secondary Discovery tab backed by `GET /discovery` (read-only).
- Start/Stop controls on project cards calling:
  - `POST /sessions/:name/start`
  - `POST /sessions/:name/stop`
- Project editor entry points:
  - `New Project` -> `POST /sessions`
  - `Edit` -> `PATCH /sessions/:name`
- Per-pane activity presentation (ATM-first runtime state from pane projection).
- CI run indicators with green/yellow/red/running semantics.

## Acceptance Criteria

- Discovery tab renders tmux-discovered sessions without mutating definitions.
- Primary dashboard view still renders all defined projects (running or stopped).
- Start/Stop controls return daemon response feedback and refresh runtime state.
- New/Edit flows can submit valid project definitions through writer-gated endpoints.
- Per-pane state is visible in cards/modal/list rows (not session-level-only badge).
- CI status indicators reflect pass/fail/running states on project cards.

## Validation

- `cargo check --workspace`
- `cargo test --workspace`
- Dashboard artifact sync:
  - `dashboard/dashboard.js` matches compiled `dashboard/team-dashboard.jsx`
  - `crates/scmux-daemon/assets/dashboard.js` matches `dashboard/dashboard.js`
