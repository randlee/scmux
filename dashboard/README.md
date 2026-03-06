# scmux Dashboard

React single-file dashboard for monitoring and jumping to tmux agent teams.

## Features

- **Grid / List / Grouped** views
- Per-session pane status (active / idle / stopped)
- Open PRs with links
- Jump modal — fires WezTerm SSH command to attach to session
- Filter by status (running / idle / stopped) and project
- Search sessions by name

## Running

The dashboard is a single React JSX file (`team-dashboard.jsx`). To run:

```bash
# Option 1: Vite
npm create vite@latest dashboard -- --template react
cp team-dashboard.jsx src/App.jsx
npm run dev

# Option 2: Serve as artifact in Claude.ai
# Paste team-dashboard.jsx contents into a Claude artifact
```

## Connecting to scmux-daemon

Replace the static `TEAMS` array with a `useEffect` fetch from your daemon:

```js
useEffect(() => {
  fetch('http://localhost:7700/sessions')
    .then(r => r.json())
    .then(setTeams);
}, []);
```

For multiple hosts, fetch from each and merge with a `host` field added to each session.

## Jump Command

The jump modal shows the WezTerm command for the selected session:

- **Local:** `wezterm start -- tmux attach -t <session-name>`
- **Remote:** `wezterm ssh user@host -- tmux attach -t <session-name>`
