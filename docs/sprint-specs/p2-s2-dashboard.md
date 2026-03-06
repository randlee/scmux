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
- Dashboard talks to the **local daemon only** — it never contacts remote hosts directly.
- Local daemon aggregates session data from remote daemons and serves everything through its own API.
- Dashboard discovers its daemon URL from `window.location.origin` (served by the daemon itself).
- Fallback for local dev: if `window.location.origin` is `file://` or `null:`, use `http://localhost:7878`.

## Data Flow

```
Dashboard (browser)
  └── talks only to local daemon at window.location.origin

On load:
  GET /dashboard-config.json
    → { hosts: [{name, is_local}], default_terminal, poll_interval_ms }

Every poll_interval_ms:
  GET /sessions         → all sessions (local + cached remote, tagged with host_id)
  GET /hosts            → reachability state per host (local daemon's view)
  merge into unified session list grouped/filtered by host
```

The local daemon handles all remote communication. The dashboard sees one unified API.

## Deliverables

### 1. `dashboard/team-dashboard.jsx`

- On load: fetch `GET /dashboard-config.json` from `window.location.origin` (fallback: `http://localhost:7878`).
- Poll loop: fetch `GET /sessions` and `GET /hosts` from local daemon every `poll_interval_ms`.
- Sessions include `host_id` — cross-reference with `/hosts` response to get `reachable` and `last_seen` per host.
- **Monochrome rendering**: sessions whose host has `reachable === false` render with `filter: grayscale(1) opacity(0.6)`.
- **Last seen indicator**: per-host group header shows "last seen X ago" when `reachable === false`, derived from `last_seen` ISO timestamp.
- **Jump modal**: "Open in iTerm2" always sends `POST /sessions/:name/jump` to the local daemon. The daemon handles routing to the correct host (local AppleScript or SSH command). Show `message` field from response as feedback. Display the exact shell command (DJ-05) in the modal.
- Preserve Grid/List/Grouped views, combinable filters (status, project, text search), header aggregate counts.
- CI badges: render `session_ci` data if present; grayed badge + install tooltip if `status = "tool_unavailable"`. Badges show nothing if no CI configured (DC-05).

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
