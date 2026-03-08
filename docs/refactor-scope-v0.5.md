# Refactor Scope — v0.5 Definition-First Architecture

This document lists code paths that conflict with the corrected architecture and should be removed or rewritten.

## 1. Must Remove

### 1.1 Discovery-to-Definition Persistence
- `crates/scmux-daemon/src/scheduler.rs`
  - NF-06 recovery path that reconstructs `sessions` from live tmux when DB is empty.
  - Any `INSERT/UPSERT` into `sessions` based on tmux discovery.

- `crates/scmux-daemon/src/hosts.rs` + `crates/scmux-daemon/src/db.rs`
  - Remote session upsert path (`upsert_remote_session`) that persists discovery snapshots.

### 1.2 Poller Runtime Writes (outside editor path)
- `crates/scmux-daemon/src/scheduler.rs`
  - writes to `session_status`, `session_events` from poll loop.
- `crates/scmux-daemon/src/ci.rs`
  - writes to `session_ci` from CI polling.
- `crates/scmux-daemon/src/atm.rs`
  - writes to `session_atm` from ATM polling.
- `crates/scmux-daemon/src/hosts.rs`
  - discovery-driven write paths for remote runtime data.

## 2. Must Constrain

### 2.1 API Write Surface
- `crates/scmux-daemon/src/api.rs`
  - only project-definition editor routes may persist to SQLite.
  - runtime control routes (`start`, `stop`, `jump`) must not perform definition writes.

### 2.2 DB Access Ownership
- Introduce a dedicated writer module for approved project edits.
- Restrict direct write helpers in `db.rs` from general use.

## 3. Rename/Responsibility Alignment

- `scheduler.rs` should be renamed to `tmux_poller.rs` (or `runtime_poller.rs`) to avoid confusion with future cron scheduler responsibilities.

## 4. Safety-Critical Behavior

- `stop` must be graceful-first:
  1. send ATM shutdown request,
  2. wait configurable grace timeout,
  3. scoped hard-stop only if needed.
- Panic/error paths must never bulk-stop unrelated sessions.

## 5. Keep and Reuse

- Existing host reachability display concepts.
- Existing dashboard embed/release automation paths.
- Existing tmux attach/jump viewer flow (iTerm as viewer only).
- Existing structured logging foundation, extended for OpenTelemetry compatibility.
