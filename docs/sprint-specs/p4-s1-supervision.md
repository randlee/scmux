# Sprint 4.1 — Supervision + Performance

- Sprint ID: `4.1`
- Worktree: `/Users/randlee/Documents/github/scmux-worktrees/feature/p4-s1-supervision`
- Base branch: `integrate/phase-4`
- PR target: `integrate/phase-4`

## Context

Functional scope is complete; operational hardening and service management are required for production reliability.

## Deliverables

1. Service assets
- `deploy/macos/com.scmux.scmux-daemon.plist` (new)
- `deploy/linux/scmux-daemon.service` (new)
- install instructions in docs.

2. Daemon status and health improvements
- `GET /health` returns structured JSON: `{status, uptime_secs, session_count, db_path, version}`.
- `scmux daemon status` prints uptime, session count, and db path (CLI-14 already wired; verify against new /health schema).
- Retention/pruning: confirm `events` table is pruned after configurable TTL; verify via `T-D-21`.

3. Performance + resilience pass
- benchmark poll cycle latency (<500ms at 50 sessions) — measured via integration test with 50 mock sessions (T-D-22).
- benchmark read endpoint latency (<100ms for GET /sessions) — measured via integration test (T-D-23).
- harden error isolation: single host or session failure must not abort poll cycle (T-D-19, T-D-20).
- NF-01 verification: add CI step `otool -L target/release/scmux-daemon` (macOS) / `ldd` (Linux) to confirm no unexpected dynamic deps beyond system libs.
- NF-02 verification: measure RSS via `ps -o rss` after loading 20 sessions; must be <50MB (T-D-24).

4. Test additions (new IDs: T-D-19..T-D-24)
- `T-D-19`: single unreachable host does not abort poll cycle
- `T-D-20`: single session failure (bad tmux state) does not abort session loop
- `T-D-21`: events table pruned after TTL; old rows absent
- `T-D-22`: poll cycle completes in <500ms with 50 mock sessions
- `T-D-23`: GET /sessions responds in <100ms
- `T-D-24`: daemon RSS <50MB after loading 20 sessions

## Acceptance Criteria

- launchd and systemd assets are functional and documented (DH-04, DH-05).
- NF-03 (T-D-22) and NF-04 (T-D-23) thresholds measured and satisfied.
- NF-02 (T-D-24): RSS <50MB verified.
- NF-08 (T-D-19, T-D-20): daemon stable under isolated failures.
- NF-06 (T-I-20): DB deleted while daemon stopped → next start reconstructs from tmux without error.
- NF-01: `otool -L`/`ldd` CI check passes.
- `cargo clippy` clean, `cargo test --workspace` passes.

## Requirement IDs Covered

- `DH-04`, `DH-05`
- `NF-01`, `NF-02`, `NF-03`, `NF-04`, `NF-05`, `NF-06`, `NF-08`
- `T-D-19..T-D-24`

## Dependencies

- Requires Sprint `3.2` merged.
- Must merge before Sprint `4.2`.
