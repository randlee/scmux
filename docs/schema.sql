-- =============================================================================
-- scmux (tmux session manager) schema
-- One SQLite database per host, located at ~/.config/scmux/scmux.db
-- =============================================================================

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- -----------------------------------------------------------------------------
-- hosts
-- The local machine plus any remote machines this dashboard knows about.
-- The local host is always present with address = 'localhost'.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS hosts (
  id           INTEGER PRIMARY KEY,
  name         TEXT    NOT NULL UNIQUE,   -- "mac-studio", "dgx-spark"
  address      TEXT    NOT NULL,          -- "localhost" or IP/hostname
  ssh_user     TEXT,                      -- NULL for localhost
  api_port     INTEGER NOT NULL DEFAULT 7700,
  is_local     BOOLEAN NOT NULL DEFAULT 0,
  created_at   DATETIME NOT NULL DEFAULT (datetime('now')),
  last_seen    DATETIME                   -- last successful health check
);

-- Every db has exactly one local host row
INSERT OR IGNORE INTO hosts (name, address, is_local)
  VALUES ('local', 'localhost', 1);

-- -----------------------------------------------------------------------------
-- sessions
-- A named tmux session config tied to one host.
-- config_json holds the full tmuxp-compatible JSON blob.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sessions (
  id            INTEGER PRIMARY KEY,
  name          TEXT    NOT NULL,         -- "ui-template" (must match tmux session name)
  project       TEXT,                     -- logical grouping, e.g. "radiant-p3"
  host_id       INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
  config_json   TEXT    NOT NULL,         -- tmuxp JSON blob
  cron_schedule TEXT,                     -- cron expression or NULL for manual-only
  auto_start    BOOLEAN NOT NULL DEFAULT 0, -- start on daemon boot if stopped
  enabled       BOOLEAN NOT NULL DEFAULT 1, -- false = daemon ignores this session
  github_repo   TEXT,                      -- "owner/repo" e.g. "randlee/scmux"
  azure_project TEXT,                      -- Azure DevOps project URL or identifier
  created_at    DATETIME NOT NULL DEFAULT (datetime('now')),
  updated_at    DATETIME NOT NULL DEFAULT (datetime('now')),

  UNIQUE (name, host_id)                  -- session names unique per host
);

-- -----------------------------------------------------------------------------
-- session_status
-- Live runtime state, updated by the daemon on each poll cycle.
-- Kept separate from sessions so the definition is never mutated by polling.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS session_status (
  session_id    INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  status        TEXT    NOT NULL DEFAULT 'stopped', -- running | stopped | starting
  panes_json    TEXT,                     -- JSON array of {name, status, last_activity}
  polled_at     DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- -----------------------------------------------------------------------------
-- session_events
-- Immutable log of start/stop transitions.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS session_events (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  event         TEXT    NOT NULL,         -- started | stopped | failed | scheduled
  trigger       TEXT    NOT NULL,         -- manual | cron | auto_start | daemon
  note          TEXT,                     -- optional detail, e.g. error message
  occurred_at   DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- -----------------------------------------------------------------------------
-- daemon_health
-- Periodic heartbeat written by the daemon. Detects if daemon died.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS daemon_health (
  id            INTEGER PRIMARY KEY,
  host_id       INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
  status        TEXT    NOT NULL,         -- ok | degraded | error
  sessions_running INTEGER,
  note          TEXT,
  recorded_at   DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- Retain 7 days of health records
CREATE INDEX IF NOT EXISTS idx_daemon_health_recorded
  ON daemon_health (recorded_at);

-- -----------------------------------------------------------------------------
-- indexes
-- -----------------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_sessions_host
  ON sessions (host_id);

CREATE INDEX IF NOT EXISTS idx_sessions_project
  ON sessions (project);

CREATE INDEX IF NOT EXISTS idx_session_events_session
  ON session_events (session_id, occurred_at);

-- -----------------------------------------------------------------------------
-- trigger: keep sessions.updated_at current
-- -----------------------------------------------------------------------------
CREATE TRIGGER IF NOT EXISTS sessions_updated_at
  AFTER UPDATE ON sessions
  FOR EACH ROW
  BEGIN
    UPDATE sessions SET updated_at = datetime('now') WHERE id = OLD.id;
  END;

-- -----------------------------------------------------------------------------
-- session_ci
-- CI/PR status per session, per provider.
-- Written exclusively by the ci_loop task in the daemon.
-- One row per (session_id, provider) pair — upserted on each poll.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS session_ci (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  provider      TEXT    NOT NULL,         -- github | azure
  status        TEXT    NOT NULL,         -- ok | tool_unavailable | error | no_config
  data_json     TEXT,                     -- JSON: PRs, pipeline runs, etc. (see below)
  tool_message  TEXT,                     -- populated when status = tool_unavailable
  polled_at     DATETIME,                 -- NULL until first successful poll
  next_poll_at  DATETIME,                 -- when ci_loop should next poll this row

  UNIQUE (session_id, provider)           -- one row per session per provider
);

-- data_json shape for provider = "github":
-- {
--   "open_prs": [
--     { "number": 42, "title": "...", "url": "...", "author": "...", "draft": false }
--   ],
--   "recent_runs": [
--     { "status": "success|failure|in_progress", "branch": "main", "name": "CI", "url": "...", "updated_at": "..." }
--   ]
-- }
--
-- data_json shape for provider = "azure":
-- {
--   "open_prs": [
--     { "id": 1, "title": "...", "url": "...", "author": "...", "status": "active" }
--   ],
--   "recent_runs": [
--     { "id": 1, "name": "...", "result": "succeeded|failed|inProgress", "branch": "main", "url": "...", "finished_at": "..." }
--   ]
-- }
--
-- data_json shape for status = "tool_unavailable":
-- null  (tool_message carries the install instruction)
--
-- data_json shape for status = "no_config":
-- null  (session has no github_repo / azure_project configured)

CREATE INDEX IF NOT EXISTS idx_session_ci_session
  ON session_ci (session_id);

CREATE INDEX IF NOT EXISTS idx_session_ci_next_poll
  ON session_ci (next_poll_at);
