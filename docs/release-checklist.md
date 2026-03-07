# Release Checklist (v0.3.0)

## Acceptance Gates

| ID | Gate | Status | Evidence |
|---|---|---|---|
| AC-01 | `cargo build --release --workspace` succeeds with no warnings | [ ] | |
| AC-02 | Daemon survives 24h on macOS without crashing (manual attestation) | [ ] | |
| AC-03 | All T-D, T-I, T-A tests pass | [ ] | |
| AC-04 | Dashboard shows real live data from daemon | [ ] | |
| AC-05 | Jump via dashboard opens iTerm2 attached to correct session | [ ] | |
| AC-06 | Killed `auto_start` session restarts within 30s | [ ] | |
| AC-07 | Cron-scheduled session starts within 15s of scheduled time | [ ] | |
| AC-08 | VPN disconnect yields monochrome remote host with no dialogs | [ ] | |
| AC-09 | VPN reconnect restores full-color within one poll cycle | [ ] | |
| AC-10 | Missing `gh`/`az` show grayed badge with install tooltip | [ ] | |

## Test Scope

- Automated E2E: `T-E-01..05`, `T-E-07`, `T-E-10`, `T-E-11`
- Manual E2E: `T-E-06`, `T-E-08`, `T-E-09`
- Perf-gate: `T-D-22`, `T-D-23` in release mode CI job

## Packaging / Release Artifacts

| Item | Status | Notes |
|---|---|---|
| Workspace version bumped to `0.3.0` | [ ] | |
| `docs/release-notes-v0.3.0.md` complete | [ ] | |
| launchd/systemd docs verified | [ ] | |
| Checksums generated for release assets | [ ] | |
| Homebrew formula checklist placeholder reviewed (non-blocking) | [ ] | Tap not created yet |
