# E2E Manual Runbook

This runbook covers manual Phase 4 E2E scenarios that are not reliable in CI.

## T-E-06: Jump Opens iTerm2 and Attaches Session

1. Start daemon and ensure `/health` is healthy.
2. Create a running session (or start an existing one).
3. Trigger jump from dashboard or CLI (`scmux jump <name>`).
4. Verify iTerm2 opens and attaches to the target tmux session.
5. Record pass/fail and capture screenshot or terminal transcript.

Expected:
- iTerm2 launches.
- Correct session is attached.
- No daemon crash or API error.

## T-E-08: VPN Disconnect Produces Monochrome Remote Host State

1. With a reachable remote host configured, open dashboard.
2. Disconnect VPN or otherwise isolate the remote host.
3. Wait one host poll interval.
4. Verify remote host sessions render in monochrome and last-seen is shown.
5. Verify no error dialogs are shown.

Expected:
- Remote host marked unreachable.
- Last-known data remains visible.
- UI degrades gracefully without blocking interaction.

## T-E-09: VPN Reconnect Restores Full-Color Remote Host State

1. Starting from T-E-08 state, reconnect VPN.
2. Wait one host poll interval.
3. Verify host reachability returns and UI color is restored.
4. Verify session data refreshes from remote host.

Expected:
- Host transitions back to reachable.
- Full-color rendering resumes automatically.
- No manual refresh is required.
