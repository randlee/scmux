# scmux Dashboard

`dashboard/team-dashboard.jsx` is the live dashboard UI for `scmux-daemon`.

## Daemon URL Discovery

The dashboard resolves the daemon base URL from `window.location.origin`.

- Normal daemon-served mode: uses the page origin directly.
- Local file/dev fallback: if origin is `null` or protocol is `file:`, uses `http://localhost:7878`.

## Multi-Host Polling Model

The browser only calls the local daemon.

- `GET /dashboard-config.json` on initial load
  - reads `default_terminal`
  - reads `poll_interval_ms`
  - seeds host metadata
- `GET /sessions` and `GET /hosts` every `poll_interval_ms`
  - sessions are cross-referenced by `host_id`
  - host reachability and `last_seen` are taken from `/hosts`
  - unreachable host sessions render monochrome in the UI

## Opening the Dashboard

Recommended path:

1. Start daemon:

```bash
scmux-daemon
```

2. Open in browser:

```text
http://localhost:7878/
```

## Local Development

You can iterate on the JSX file with a static server while keeping daemon APIs running:

```bash
cd dashboard
python3 -m http.server 8080
```

Then open `http://localhost:8080/` and ensure a daemon is running on `http://localhost:7878` (fallback URL), or serve from daemon origin directly.
