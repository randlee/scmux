# E2E Environment

This document defines the baseline environment for running Phase 4 end-to-end validation.

## Host Requirements

- macOS 14+ (primary validation host)
- Linux host for cross-platform smoke checks (optional for local E2E, required for release matrix)
- Rust stable toolchain

## Toolchain Prerequisites

- `tmux >= 3.3`
- `tmuxp >= 1.30`
- `iTerm2 >= 3.5` (macOS manual jump validation)
- `gh` CLI authenticated (`gh auth status`)
- `az` CLI available if Azure CI sessions are configured

## Runtime Configuration

- `SCMUX_PORT=7878` (default)
- `SCMUX_DB` points to writable path
- `SCMUX_LOG=info` (or `debug` for troubleshooting)

## Automated Test Setup Notes

- E2E automated tests use fake binaries via:
  - `SCMUX_TMUX_BIN`
  - `SCMUX_TMUXP_BIN`
  - `SCMUX_OSASCRIPT_BIN` (macOS-only jump path tests)
- Perf-gate benchmarks run separately in `--release`.
- VPN-related tests are manual only (`T-E-08`, `T-E-09`).
