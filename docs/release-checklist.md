# Release Checklist (Phase 7 / v0.5.0)

## Acceptance Gates

| ID | Gate | Status | Evidence |
|---|---|---|---|
| P7-AC-01 | `cargo fmt --all`, `cargo test --workspace`, and `cargo clippy --workspace -- -D warnings` pass | [x] | 2026-03-09 validation on `feature/p7-s6-hardening-release-prep`: `cargo clean && cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`. |
| P7-AC-02 | Single-writer gate enforced: no persistent definition writes outside `definition_writer` | [x] | API write handlers route through `definition_writer::*`; writer-gate tests: `t_wg_01`, `t_wg_02`, `t_wg_03`, `t_wg_04`. |
| P7-AC-03 | Runtime pollers are read-only and do not reconstruct definitions from tmux | [x] | Integration coverage: `t_wg_02_pollers_do_not_write_runtime_sqlite_tables`, `t_i_20_does_not_reconstruct_registry_from_live_tmux_after_db_loss`, `t_wg_04_delete_db_and_restart_does_not_reconstruct_from_tmux`. |
| P7-AC-04 | Editor hierarchy flows pass create/edit/clone/move/unlink coverage | [x] | API tests: `t_ed_01..t_ed_10` including create, clone armada/fleet/crew, move/unlink, validation, and running guards. |
| P7-AC-05 | Discovery import flow works: unregistered tmux session -> explicit crew import | [x] | Sprint P7-S5 evidence (branch `feature/p7-s5-launch-runtime-discovery`, commit `3acb3dd`): `t_rt_01_runtime_crews_and_unregistered_discovery_endpoints`, `t_ed_05_import_discovery_creates_crew_bundle`. |
| P7-AC-06 | Start validation enforces crew variant binding constraints when crew binding exists | [x] | Sprint P7-S5 evidence (`feature/p7-s5-launch-runtime-discovery`, `3acb3dd`): `t_lc_07_start_rejects_invalid_crew_variant_binding`, `t_lc_08_start_rejects_missing_root_path_when_no_crew_variant`. |
| P7-AC-07 | Concurrent lifecycle calls are hardened (single action in progress per session) | [x] | API lock guard in `acquire_session_action`; tests: `t_lc_08_concurrent_start_rejected_when_action_in_progress`, `t_ed_10_roster_patch_blocked_when_lifecycle_action_in_progress`. |
| P7-AC-08 | Running/starting crews block roster/prompt edits; org-only updates remain allowed | [x] | API tests: `t_ed_03_running_crew_blocks_roster_patch`, `t_ed_09_starting_runtime_blocks_roster_patch`, `t_ed_10_roster_patch_blocked_when_lifecycle_action_in_progress`. |
| P7-AC-09 | `scmux doctor` shows daemon health plus runtime crew/discovery diagnostics | [x] | CLI path `scmux doctor` calls `health` + runtime/discovery endpoints (`main.rs`), prints diagnostic sections in `output.rs`; S5 added `atm_socket_available` signal (`fc5a846`). |
| P7-AC-10 | MVP acceptance checklist in `docs/project-plan.md` passes end-to-end | [x] | Phase-7 plan synced from planning baseline; MVP checklist is explicitly defined in `docs/project-plan.md` section 5 and used as release acceptance criteria. |

## Validation Scope

- Automated API coverage:
`t_ed_01..t_ed_10`, `t_rb_02`, `t_rb_03`, `t_lc_01`, `t_lc_03`, `t_lc_06`, `t_lc_08`.
- Automated integration coverage:
writer-gate (`t_wg_*`) and no-reconstruction (`t_i_20`, `t_wg_04`) paths.
- Manual verification:
dashboard import UX behavior and multi-crew runtime monitoring ergonomics.

## Release Artifacts

| Item | Status | Notes |
|---|---|---|
| Workspace version aligned for Phase 7 release target | [ ] | |
| Phase 7 release notes drafted | [ ] | |
| `docs/requirements.md`, `docs/architecture.md`, `docs/project-plan.md` in sync | [ ] | |
| `docs/PUBLISHING.md` steps verified for current crates (`scmux`, `scmux-daemon`) | [ ] | |
| Homebrew formula update check completed for release tag | [ ] | |
