# Sprint 2.2 — Live Dashboard

- Sprint ID: `2.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s2-dashboard`
- Base branch: `feature/p2-s1-multihost`
- PR target: `integrate/phase-2`

## Context

Dashboard currently uses mock session data and does not consume host-aware daemon responses.

## Deliverables

1. `dashboard/team-dashboard.jsx`
- Replace static `TEAMS` usage with polling fetch logic from daemon host endpoints.
- Preserve existing layout structure; do not redesign unless required for functionality.
- Add host-aware rendering model:
  - `reachable: false` → monochrome session cards/rows.
  - `last_seen` indicator for unreachable hosts.
- Wire jump modal action to `POST /sessions/:name/jump`.
- Keep Grid/List/Grouped views and combinable filters.

2. `dashboard/README.md`
- Document live API wiring and host polling assumptions.

3. UI validation checklist doc/update
- Add explicit checklist for `T-UI-01..T-UI-17` execution.

## Acceptance Criteria

- Dashboard displays live session + host data from daemon APIs.
- Unreachable hosts render in monochrome with last-seen indicator.
- Jump modal issues daemon jump request and shows feedback.
- All UI view/filter behaviors match requirements.

## Requirement IDs Covered

- `DV-01..DV-11`
- `DC-01..DC-05`
- `DJ-01..DJ-07`
- `MH-06`, `MH-07`, `MH-08`
- `T-UI-01..T-UI-17`

## Dependencies

- Requires Sprint `2.1` merged.
- Must merge before Sprint `3.2` acceptance checks.
