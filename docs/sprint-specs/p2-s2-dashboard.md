# Sprint 2.2 — Live Dashboard

- Sprint ID: `2.2`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p2-s2-dashboard`
- Base branch: `feature/p2-s1-multihost`
- PR target: `integrate/phase-2`

## Context

Dashboard is a single JSX file (`dashboard/team-dashboard.jsx`) served statically by the daemon's `GET /`
route. It currently uses hardcoded mock session data (`TEAMS` constant). It needs to switch to live data
from the daemon APIs, with host-aware rendering for unreachable hosts.

## Tech Stack

- Plain React JSX loaded via browser-native ESM `import` — no build step, no bundler.
- Served by `axum` static file handler at `GET /`.
- Dashboard discovers its daemon URL from `window.location.origin` (it is served by the daemon itself).
- For multi-host: fetch `GET /dashboard-config.json` on load to get all host URLs and poll interval.

## Data Flow

```
On load:
  fetch GET /dashboard-config.json
    → { hosts: [{name, url}], poll_interval_ms }

Every poll_interval_ms:
  for each host:
    fetch host.url + GET /sessions  → session list with status/panes
    fetch host.url + GET /hosts     → reachability state
  merge all sessions into unified list tagged with host name + reachable flag
```

## Deliverables

### 1. `dashboard/team-dashboard.jsx`

- On load: fetch `GET /dashboard-config.json` to get host list and poll interval.
- Poll loop: fetch `/sessions` and `/hosts` from each host URL every `poll_interval_ms`.
- Tag each session with `hostName` and `reachable` flag from the hosts response.
- **Monochrome rendering**: sessions where `reachable === false` render with CSS `filter: grayscale(1) opacity(0.6)`.
- **Last seen indicator**: unreachable host header or session group shows "last seen X ago" derived from `last_seen` ISO timestamp.
- Wire jump modal: "Open in iTerm2" button sends `POST /sessions/:name/jump` to the correct host URL. Show `message` field from response as feedback.
- Preserve existing Grid/List/Grouped views, combinable filters (status, project, text search), and header aggregate counts.
- CI badges: show `session_ci` data if present; grayed badge + tooltip if `status = "tool_unavailable"`.

### 2. `dashboard/README.md`

Document:
- How the dashboard discovers its daemon URL
- Multi-host polling model
- How to open dashboard (serve via daemon, open browser)
- How to run locally for development (e.g. `python3 -m http.server` with a running daemon)

### 3. `docs/sprint-specs/p2-s2-dashboard-test-checklist.md`

Manual test checklist mapping T-UI-01..T-UI-17 to explicit steps. Each item includes:
- Precondition
- Action
- Expected result

## Acceptance Criteria

- Dashboard loads and displays live session + host data from daemon APIs without any build step.
- Unreachable host sessions render in monochrome (`filter: grayscale`) with last-seen indicator.
- Jump modal sends `POST /sessions/:name/jump` to the correct host and shows daemon response feedback.
- Grid, List, and Grouped views all work correctly with live data.
- Status, project, and text search filters work and are combinable.
- Header counts (running/idle/stopped/agents/PRs) reflect live data.
- T-UI-01..T-UI-17 checklist manually verified.

## Requirement IDs Covered

- `DV-01..DV-11`
- `DC-01..DC-05`
- `DJ-01..DJ-07`
- `MH-06`, `MH-07`, `MH-08`
- `T-UI-01..T-UI-17`

## Dependencies

- Requires Sprint `2.1` merged.
- Must merge before Sprint `3.2` acceptance checks.
