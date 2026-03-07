# Sprint 2.2 Dashboard Manual Test Checklist

This checklist maps `T-UI-01..T-UI-17` to explicit manual verification steps.

## T-UI-01 Grid view renders all sessions
- Precondition: daemon running with at least 3 sessions.
- Action: open dashboard, click `Grid` view.
- Expected: one card per session currently returned by `GET /sessions`.

## T-UI-02 List view renders all sessions in table
- Precondition: daemon running with at least 3 sessions.
- Action: click `List` view.
- Expected: table rows match session count from `GET /sessions`.

## T-UI-03 Grouped view groups by project
- Precondition: sessions exist across at least 2 projects.
- Action: click `Project` view.
- Expected: sessions are grouped under project headers with per-project summary counts.

## T-UI-04 Status filters work correctly
- Precondition: dataset includes `running`, `idle`, and `stopped` sessions.
- Action: click each status filter (`running`, `idle`, `stopped`).
- Expected: only sessions with selected status remain visible.

## T-UI-05 Project filter shows only correct sessions
- Precondition: sessions exist in at least 2 projects.
- Action: select one project filter chip.
- Expected: only sessions for that project are shown.

## T-UI-06 Search filters by name substring
- Precondition: at least one session has a distinct name substring.
- Action: type substring in search input.
- Expected: visible sessions are limited to names containing substring (case-insensitive).

## T-UI-07 Clicking session opens jump modal
- Precondition: at least one session is visible.
- Action: click any session card/row.
- Expected: jump modal opens for that session.

## T-UI-08 Modal shows correct pane list
- Precondition: target session has pane data in `panes` array.
- Action: open jump modal for target session.
- Expected: pane names and statuses match current session pane payload.

## T-UI-09 Modal shows correct PR badges with links
- Precondition: target session has GitHub PR metadata in session CI payload.
- Action: open jump modal.
- Expected: PR badges/list show expected PR numbers/titles and links open in new tab.

## T-UI-10 Modal "Open in iTerm2" sends POST /jump and shows feedback
- Precondition: daemon reachable; target session exists.
- Action: click `Open in iTerm2 ->` in modal.
- Expected: `POST /sessions/:name/jump` is sent to daemon, and modal displays daemon `message` response.

## T-UI-11 Stopped sessions are visually de-emphasized
- Precondition: at least one session status is `stopped`.
- Action: observe stopped session in any view.
- Expected: stopped session has reduced visual emphasis (lower opacity).

## T-UI-12 Unreachable host sessions render in monochrome
- Precondition: at least one host is reported `reachable=false` by `GET /hosts`.
- Action: observe sessions mapped to that host.
- Expected: those sessions render with grayscale/monochrome styling.

## T-UI-13 "Last seen N ago" shows for unreachable hosts
- Precondition: unreachable host has non-null `last_seen`.
- Action: inspect grouped host header and/or session host status labels.
- Expected: text shows `last seen <relative time>`.

## T-UI-14 Full color resumes when host returns
- Precondition: host transitions from unreachable to reachable in `/hosts`.
- Action: wait for next poll interval or trigger refresh.
- Expected: sessions for that host return to full color rendering.

## T-UI-15 Tool-unavailable CI badges show install tooltip
- Precondition: session CI entry has `status="tool_unavailable"` and message.
- Action: hover tool-unavailable badge.
- Expected: tooltip displays install/help message from daemon payload.

## T-UI-16 Escape key closes modal
- Precondition: jump modal is open.
- Action: press `Escape`.
- Expected: modal closes.

## T-UI-17 Header counts match data
- Precondition: sessions include mixed statuses and pane activity.
- Action: compare header counters with computed values from current sessions payload.
- Expected: running/idle/stopped/active-agents/open-PR counts are correct.
