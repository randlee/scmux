# Release Checklist (Phase 7 / v0.5.0)

## Acceptance Gates

| ID | Gate | Status | Evidence |
|---|---|---|---|
| P7-AC-01 | `cargo fmt --all`, `cargo test --workspace`, and `cargo clippy --workspace -- -D warnings` pass | [ ] | |
| P7-AC-02 | Single-writer gate enforced: no persistent definition writes outside `definition_writer` | [ ] | |
| P7-AC-03 | Runtime pollers are read-only and do not reconstruct definitions from tmux | [ ] | |
| P7-AC-04 | Editor hierarchy flows pass create/edit/clone/move/unlink coverage | [ ] | |
| P7-AC-05 | Discovery import flow works: unregistered tmux session -> explicit crew import | [ ] | |
| P7-AC-06 | Start validation enforces crew variant binding constraints when crew binding exists | [ ] | |
| P7-AC-07 | Concurrent lifecycle calls are hardened (single action in progress per session) | [ ] | |
| P7-AC-08 | Running/starting crews block roster/prompt edits; org-only updates remain allowed | [ ] | |
| P7-AC-09 | `scmux doctor` shows daemon health plus runtime crew/discovery diagnostics | [ ] | |
| P7-AC-10 | MVP acceptance checklist in `docs/project-plan.md` passes end-to-end | [ ] | |

## Validation Scope

- Automated API coverage:
`t_ed_01..t_ed_06`, `t_rt_01`, `t_lc_01`, `t_lc_03`, `t_lc_06`, `t_lc_07`, `t_lc_08`.
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
