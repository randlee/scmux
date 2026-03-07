# scmux v1.0.0 Release Notes

## Summary

`v1.0.0` is the first production release of scmux: a daemon-backed tmux session manager for multi-agent workflows across local and remote hosts.

## Highlights Since v0.x

- Daemon-managed SQLite session registry and status tracking
- HTTP API for session lifecycle, health, host state, and dashboard config
- CLI client (`scmux`) for list/show/start/stop/jump/add/edit/remove flows
- Dashboard with status views, filtering, CI badges, and jump modal
- Multi-host reachability handling with monochrome degradation for unreachable hosts
- CI integration for GitHub/Azure provider status (with graceful missing-tool behavior)
- launchd/systemd service assets and deployment guide
- Supervision/perf hardening and Phase 4 E2E coverage

## Known Limitations

- iTerm2 jump automation is macOS-only
- VPN disconnect/reconnect UX validation requires manual runbook execution
- Homebrew tap/formula is not yet published (tracked as post-release checklist item)

## Install Notes

- macOS launchd template: `deploy/macos/com.scmux.scmux-daemon.plist`
- Linux systemd template: `deploy/linux/scmux-daemon.service`
- Deployment instructions: `docs/deploy.md`

## Upgrade Path

No migration path is required for `v1.0.0` because this is the first public stable release.
