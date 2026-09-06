===== ap-84f =====
TITLE: hmux: duplicate rmux as a Python herdr launcher (MVP)
# hmux — duplicate rmux functionality as a Python herdr launcher (MVP)

## Goals

1. **Duplicate `scripts/rmux` functionality with `hmux`** — a Python CLI backed
   by herdr instead of tmux, usable immediately.
2. **Reuse the agent-launching code** — extract config parsing + agent-command
   derivation into a shared Python module so it is NOT duplicated for hmux
   (and is available to rmux later).
3. **Tests covering all edge and corner cases** — the launch-core logic is the
   part that historically lived as subtle zsh; pin it down with a test suite.
4. **Integration tests against a fast low-cost agent (`Luna`)** — no real
   (expensive) agent. Launch a session with Luna as the stand-in and use the
   hmux nudge commands to talk to it, proving the command lines work end-to-end.

## Deliverables

1. `hmux` — Python CLI, drop-in for `rmux` (same commands, same `.atm.toml`),
   herdr backend. Installed alongside `rmux` (`~/.local/bin/hmux`).
2. Shared launch-core module (config + agent derivation) — single source,
   imported by hmux, reusable by rmux/scmux-daemon later.
3. Test suite covering edge/corner cases (team-lead vs secondary, trio vs
   no-trio, missing team, model-vs-command, .env presence, prompt append).
4. Integration test: launch a session with `Luna` (fast/low-cost stand-in),
   nudge it over herdr, confirm round-trip — no real agent cost.
5. herdr-native nudge path so `atm send <agent>` wakes the right pane.
6. Working proof: from a non-TTY shell, `hmux` launches a scratch team into
   herdr and a nudge reaches the correct agent.

## Ratified rulings (authoritative — Rand)

- **workspace = ATM_TEAM, pane = ATM_IDENTITY**, across the board.
- **Reuse `[rmux]` attributes verbatim** — no new config attributes.
- **Python** — duplicate rmux's logic; NOT Rust. Rust is deferred to the future
  scmux-daemon integration (over UDS/TCP), which is NOT MVP.
- **Launch/nudge via raw pane surface** (`herdr pane run`), never `agent start`
  (hermes v0.20 detection manifest mismatch).
- **Claude flags:** drop `--teammate-mode tmux` (tmux integration won't work
  under herdr); KEEP the undocumented trio `--agent-id/--agent-name/--team-name`.

## Identity-resolution ruling (reconciling `[rmux].session` vs ATM_TEAM)

- **Workspace label is ATM_TEAM (authoritative).** `[rmux].session` is a tmux
  session name and has NO herdr role: it is READ (for verbatim config compat)
  but IGNORED for the workspace key.
- If `[rmux].session` disagrees with the resolved ATM_TEAM, ATM_TEAM wins
  silently — no error. (The session name was never a second source of truth;
  it just happened to equal the team name in tmux.)
- Covered by a dedicated test in S-2.

## Pane identity ruling — TWO distinct concepts + ONE canonical function name

A `[rmux]` pane carries `name` (config key), `agent` (explicit field), and
`env.ATM_IDENTITY`. These feed TWO different purposes; do NOT conflate them.
**Canonical function names (cite these exact symbols everywhere):**
- `resolve_trio_identity(pane)` — claude trio name.
- `resolve_pane_label(pane)` — herdr pane label. (No other name for this
  function exists; do not call it `resolve_pane_identity`.)

### 1. Trio identity (claude `--agent-id/--agent-name/--team-name`)

**Faithful to rmux:377-384 exactly.** Precedence:

1. explicit `agent` field (rmux:377-378)
2. `env.ATM_IDENTITY`, when non-empty AND ≠ `team-lead` (rmux:380-383)
3. otherwise **none** → no trio flags (team-lead / unconfigured secondary)

### 2. Pane label (herdr `pane rename` + nudge lookup key) — `resolve_pane_label`

The herdr pane label needs a stable non-empty value for EVERY pane (including
team-lead). Precedence:

1. `env.ATM_IDENTITY` (when present — `team-lead` is a valid label here)
2. `agent` field
3. `name`

This honors Rand's "pane = ATM_IDENTITY" ruling (ATM_IDENTITY is the primary
label key) while the trio identity stays faithful to rmux's `agent → env` order.

### `hmux launch <pane-name>` match

- **config lookup by `name`** — find the `[rmux]` pane whose `name == <pane-name>`
  (unchanged from rmux:470-472).
- **runtime match by label** — that pane's label (`resolve_pane_label`).

Covered by S-2 tests for both `agent`-wins-over-`env.ATM_IDENTITY` (trio) and
`name != ATM_IDENTITY` (label).

## `[rmux]` layout ruling (mirrors the session ruling)

`[rmux] windows[].layout` is a tmux layout name (`tiled`, `even-horizontal`,
…) with NO herdr analogue (herdr splits are direction+ratio based). It is READ
but IGNORED. herdr split strategy is deterministic and stated in S-4.

## Team resolution — mode-specific (faithful to rmux source)

**This is the SINGLE authoritative statement for the hmux launch-core.** S-1
and S-2 reference it; do not restate the full orders elsewhere.

rmux uses TWO different resolution orders. hmux duplicates them exactly:

- **spawn mode** (`hmux <type> <name>`): `--team` flag → `[core].default_team`
  → ambient `ATM_TEAM` env var. (rmux:56-62 — uses `[core]`, NOT `[atm]`, NOT
  the `.env` file.)
- **session mode** (`hmux` / `hmux launch`, per-pane team for the claude trio):
  pane `env.ATM_TEAM` → `.env`-file `ATM_TEAM=` (config dir) → `[atm].default_team`.
  (rmux:306-313 + 387-388 — uses `.env` file + `[atm]`, NOT ambient env, NOT `[core]`.)

The spawn/session asymmetry (`[core]` vs `[atm]`) is a faithful rmux quirk.
Duplicated as-is for parity; flagged here so Rand can reconcile it later if
desired — reconciling is OUT of MVP scope.

**Note — the nudge script has its OWN team resolution**, distinct from
`hmux_core.resolve_team` and OUTSIDE this ruling: payload team → `.atm.toml`
walk → ambient env (the existing `atm-nudge-xml-1.py` behavior, S-6). Do not
conflate it with the launch-core's mode-specific order.

## Sibling-dependency relations (must_follow / parallel_safe)

- S-2 (tests) `must_follow` S-1 (module) — tests import the module.
- S-3 (herdr wrapper) `parallel_safe` with S-1/S-2 — no module/artifact overlap.
- S-4 (session builder) `must_follow` S-1 + S-3.
- S-5 (launch/spawn) `must_follow` S-4.
- S-6 (nudge) `must_follow` S-3 (uses the pane surface).
- S-7 (integration) `must_follow` S-4 + S-6.

## Evidence base

- `ap-1cp.1` (empirical herdr deep-dive) — the mapping + headless findings.
- grecon's herdr docs (github-research/herdr/, incl. cli.html ATM mapping).
- `scripts/rmux` (the behavior being duplicated).
- `~/.local/bin/atm-nudge-xml-1.py` (the XML envelope being reused — see S-6).

## Out of scope (explicit)

- No scmux-daemon integration (UDS/TCP) — future, not MVP.
- No agent-aware surface (`agent prompt --wait`) — blocked on ap-1cp.2.
- No reconcile/robustness features beyond what rmux already does.
- No reconciling the spawn/session team-resolution asymmetry — faithful parity only.


===== ap-84f.1 =====
TITLE: Feature: Python hmux CLI (drop-in for rmux, herdr backend)
# Feature — Python hmux CLI

Duplicate rmux's command surface exactly, with a herdr backend.

```
hmux                              # full launch (session + windows + panes)
hmux launch <pane-name>           # launch/restart one pane by name
hmux <type> <name> [OPTIONS]      # spawn ad-hoc agent
```

`<type>` in codex|claude|sonnet|haiku|opus|fable|gemini|hermes.
Options: `--config`, `--dry-run`, `-v/--verbose`, `-h/--help` (spawn adds `--team/--model/--window`).

Reads `.atm.toml` `[rmux]` verbatim; binary name (`hmux`) is the only
discriminator from `rmux`. All launch logic lives in the shared launch-core
module (S-1); the CLI is a thin arg-parser + orchestrator over it.

**"drop-in" means CLI-surface + config parity.** It does NOT mean bit-identical
internals. The tmux session/window/pane model is replaced by herdr
workspace/tab/pane; the claude `--teammate-mode tmux` flag is intentionally
dropped (ruled). See epic for the full parity scope.


===== ap-84f.1.1 =====
TITLE: hmux S-1: Shared launch-core module
# S-1 — Shared launch-core module (Python)

Extract config parsing + agent-command derivation into a reusable Python module
(`hmux_core`), so the launch logic is NOT duplicated.

## Public API (explicit signatures — the module boundary)

```python
def load_config(config_path: str) -> RmuxConfig            # raises ConfigError
def resolve_team(cfg, mode, cli_team=None, env=os.environ) -> str   # raises TeamError
def resolve_trio_identity(pane: Pane) -> str | None        # agent field -> env.ATM_IDENTITY (skip team-lead) -> None  [rmux:377-384]
def resolve_pane_label(pane: Pane) -> str                  # env.ATM_IDENTITY -> agent -> name   [herdr label]
def build_init_cmd(pane, config_dir, mode) -> str           # mode: "spawn" | "session"
def build_pane_commands(pane: Pane, team: str) -> str       # session + launch modes (config pane)
def build_spawn_commands(team, spawn_type, spawn_name, model=None, parent_session_id=None) -> str
def dry_run_projection(cfg, team, *, mode) -> str           # matches rmux --dry-run (except deltas, see S-2)
def resolve_binary(name) -> str                             # whence -p, fallback name
def env_file_path(config_dir) -> str                        # config_dir/.env
```

Dataclasses: `RmuxConfig` (session, windows[], panes[]), `Pane` (name, dir,
env, model, command, prompt, agent). Errors: `ConfigError` (missing/malformed
config, no `[rmux]`), `TeamError` (team unresolvable). No herdr/tmux import.

**Two identity functions — do NOT conflate (see epic "Pane identity" ruling):**
- `resolve_trio_identity(pane)` → claude trio name. Faithful to rmux:377-384:
  `agent` field first, then `env.ATM_IDENTITY` (non-empty and ≠ `team-lead`),
  else `None` (no trio).
- `resolve_pane_label(pane)` → herdr pane label (rename + nudge key): always
  non-empty: `env.ATM_IDENTITY` (including `team-lead`) → `agent` → `name`.

**Mode taxonomy (all three CLI modes covered):**
- `build_pane_commands(pane, team)` → **session** mode AND **launch** mode.
  These share ONE derivation in rmux (`build_pane_commands`, rmux:356-404,
  "shared by session + launch modes"). `hmux launch` uses this function.
- `build_spawn_commands(...)` → **spawn** mode only (rmux:99-129, the ad-hoc
  `hmux <type> <name>` path). No config `Pane` exists for a spawn; the inputs
  are the team + spawn type/name + optional model + leadSessionId.

## Behavior (faithful to rmux)

- Read `[rmux]` (session, windows, panes: name/dir/env/model/command/prompt/agent) + `default_team` from `[atm]` and `[core]`.
- `resolve_team` is MODE-SPECIFIC — **see epic "Team resolution" ruling** (single source; not restated here).

### Init command (MODE-SPECIFIC — split, not one template)

**spawn mode** (rmux:99-103):
```
cd <dir>; [set -a; source .env; set +a; hash -r;] export ATM_IDENTITY=<name>; export ATM_TEAM=<team>
```
(the `.env` block appears only if `<dir>/.env` exists)

**session mode** (rmux:335-348):
```
cd <dir>; [set -a; source .env; set +a; hash -r;] export <k1>=<v1>; export <k2>=<v2>; ...
```
where the `export` list is EVERY key in the pane's `env` table — NOT an
unconditional `ATM_IDENTITY/ATM_TEAM`. Those two are exported only when present
in the pane's `env`. A pane with custom (non-ATM) env keys must keep them.

### Agent command (session/launch, model pane → claude) — `build_pane_commands`

```
claude --model <m> --dangerously-skip-permissions [--agent-id <n>@<team> --agent-name <n> --team-name <team>]
```
- `agent_name` = `resolve_trio_identity(pane)` (agent field → env.ATM_IDENTITY, skip team-lead).
- trio present for secondary agents; ABSENT for team-lead.
- `--teammate-mode tmux` DROPPED (ruled). No `--parent-session-id` in session/launch mode (that is spawn-only).
- `atm_team` for the trio = pane `env.ATM_TEAM` → default team (rmux:387-388).

### Agent command (session/launch, raw command pane)

Passthrough: codex/gemini/hermes/empty shell (no claude wrapper).

### Spawn-mode command table (`build_spawn_commands`) — ALL 8 types (rmux:73-79,105-128)

| type | default model (if --model absent) | command |
|---|---|---|
| codex | — | `codex -c features.codex_hooks=true --yolo` |
| gemini | — | `gemini` |
| hermes | — | `<hermes-bin> --profile <name> [-m <model>]` |
| claude | `sonnet` | `<claude-bin> --model <m> --dangerously-skip-permissions --agent-id <n>@<t> --agent-name <n> --team-name <t> [--parent-session-id <id>]` (no `--teammate-mode`) |
| sonnet | `sonnet` | same as claude |
| haiku | `haiku` | same as claude |
| opus | `opus` | same as claude |
| fable | `fable` | same as claude |

Default-model resolution (rmux:73-79): `claude|sonnet → sonnet`, `haiku → haiku`,
`opus → opus`, `fable → fable`; `codex|gemini` no default; `hermes` no default
model (only `-m` when passed). `--parent-session-id` appended when the team
config provides `leadSessionId` (rmux:82-93,125-127).

- Prompt append: raw trailing arg (see S-2 quoting test).
- Locate `.env` (config dir), resolve `claude`/`hermes` binaries.

**Done when:** pure functions with no herdr/tmux dependency; the signatures
above importable and unit-tested; single import point for hmux (and later
rmux + scmux-daemon).


===== ap-84f.1.2 =====
TITLE: hmux S-2: Launch-core test suite (edge + corner cases)
# S-2 — Launch-core test suite (edge + corner cases)

Pin down the shared module with unit tests covering all edge and corner cases.

## Test matrix

- team-lead vs secondary agent (trio present/absent)
- `--teammate-mode tmux` NEVER emitted (tmux-lie)
- **trio identity precedence (faithful to rmux:377-384):** `agent` field wins over `env.ATM_IDENTITY`; when `agent` absent, `env.ATM_IDENTITY` (non-empty, ≠ team-lead); when both absent → no trio
- **pane label precedence:** `env.ATM_IDENTITY` → `agent` → `name`; and a case where **`name != ATM_IDENTITY`** → label = ATM_IDENTITY (not `name`)
- model pane vs raw `command` pane (claude vs codex/gemini/hermes)
- **spawn team resolution** and **session team resolution** — per epic ruling (single source)
- **spawn mode does NOT read `[atm].default_team`** (negative test — pins the spawn/session asymmetry so it can't silently drift)
- **`.env` file defines ATM_TEAM, shell does NOT** → session mode still resolves it
- **`[rmux].session` != ATM_TEAM** → workspace keyed by ATM_TEAM, session ignored (no error)
- **spawn default-model resolution** — one test per type row of the S-1 table (claude|sonnet→sonnet, haiku→haiku, opus→opus, fable→fable; codex/gemini/hermes no default)
- **session pane with custom (non-ATM) env keys** → init command exports every pane.env KV, not just ATM_IDENTITY/ATM_TEAM
- missing ATM_TEAM (error path, both modes)
- `.env` present vs absent (init command source)
- `[atm]` vs `[core]` default_team (used by the CORRECT mode)
- prompt append (model + raw), incl. multi-word prompt quoting
- empty panes / window with no panes
- dir default vs explicit

## dry-run parity gate (explicit deltas — NOT literal field-for-field)

`dry_run_projection(cfg, team, mode)` output matches `rmux --dry-run`
field-for-field **EXCEPT for the following enumerated deltas**:

1. every claude command lacks `--teammate-mode tmux` (ruled drop)
2. the session/workspace key is ATM_TEAM, whereas rmux reports `[rmux].session`
3. herdr pane ids (`wN:pN`) where rmux reports tmux `%N` ids (S-4 output only)

Anything not in that list must match. A new expected delta MUST be added to
this list before the gate is considered satisfied.

**Done when:** tests green; `dry_run_projection` against a real `.atm.toml`
matches `rmux --dry-run` modulo ONLY the enumerated deltas.


===== ap-84f.1.3 =====
TITLE: hmux S-3: herdr backend module
# S-3 — herdr backend module

Thin wrapper over the `herdr` CLI (subprocess), isolating all herdr calls.

- workspace: `create --label <team>` (`.result.tab.workspace_id` / `.result.root_pane.pane_id`), `list`, `close`.
- tab: `create --workspace <id> --label <name>` (`.result.tab.tab_id`).
- pane: `split --pane <id> --direction right|down --cwd --env` (`.result.pane.pane_id`), `rename <id> <label>`, `list --workspace <id>` (pane_id + label), `run <id> <cmd>`, `send-text <id> <text>`, `read <id> --source recent-unwrapped`.
- **Pane label = `resolve_pane_label(pane)` output** (three-level precedence:
  `env.ATM_IDENTITY` → `agent` → `name`; see epic "Pane identity" ruling). The
  wrapper renames a pane to whatever label the CALLER computes — it does NOT
  hardcode `ATM_IDENTITY`. Workspace label = ATM_TEAM.

**Primitive usage (resolves which primitive for which job):**
- `pane run <id> <cmd>` — launches a command (sends text + Enter, shell-executed). Used for agent launch + init.
- `pane send-text <id> <text>` — literal text, NO Enter. Used only when raw literal delivery is needed (not the nudge).
- The ATM nudge uses `pane run` (it must submit the envelope, not type literal text without Enter) — see S-6.

## Sentinel capture (OWNED here — referenced by S-4 and S-7)

During the live-herdr exercise, launch one pane per agent type (hermes, codex,
claude, gemini) and capture the ACTUAL prompt strings they render. Also confirm
whether the stand-in agent's ack path echoes the message-id to the pane (S-7's
nonce assertion depends on it — see S-7). Record both back into:
- the S-4 readiness table (anchored prompt regex), and
- the S-7 assertion (exact codex marker + ack-echo behavior).

This capture is a S-3 deliverable, not an ad-hoc side effect — S-4's HARD GATE
and S-7's assertion both depend on it, so it must not silently slip.

**Done when:** module exercises every call against the live herdr server and
returns parsed IDs; AND the observed prompt strings for hermes/codex/claude/
gemini are captured and back-filled into the S-4 readiness table and the S-7
assertion (including the ack-echo observation).


===== ap-84f.1.4 =====
TITLE: hmux S-4: Session builder (full launch)
# S-4 — Session builder (full launch)

`hmux` (no args): create team workspace + tabs + panes + launch each agent.

- find-or-create workspace by label == ATM_TEAM (NOT `[rmux].session` — see epic ruling).
- `[rmux] windows[].layout` is READ but IGNORED (no herdr analogue). Split
  strategy: for each window after the root, alternate `--direction right` then
  `--direction down` for successive panes (deterministic, ratio default).
- per window: tab (first = workspace root, else `tab create`); per pane: `pane split` (root reused), rename to the pane's label via `resolve_pane_label(pane)` (see epic "Pane identity" ruling), cwd=dir + env.
- launch each pane: `pane run "<init>; <agent-cmd>"` (agent-cmd from `build_pane_commands`).

## Readiness assertion (discriminating, NOT a bare `>`)

`pane read --source recent-unwrapped` readiness is asserted as follows:

- **Match against the TAIL of the buffer**, not anywhere: take the last N lines
  (N=8) of `recent-unwrapped` output and match against those lines only. A stray
  `>` in mid-buffer output does NOT satisfy readiness.
- **Anchored prompt form**, per agent type:

| agent | prompt regex (anchored at line start) |
|---|---|
| hermes | `^\s*[^ ]+ ❯\s*$` (e.g. `alpha-prime ❯`) |
| codex | `^>\s*$` (bare `>` on its own line, nothing after) |
| claude (sonnet/haiku/opus/fable) | `^>\s*$` (bare `>` on its own line) |
| gemini | `^>\s*$` (bare `>` on its own line) |

- **HARD GATE: S-4 does not close until the actual observed prompt strings are
  recorded back into this table during S-3's live-herdr exercise.** The
  regexes above are the target shape; the literal strings (what claude/codex/
  gemini actually render) are captured empirically in S-3 and back-filled here
  before S-4 closes. The bare `>` alone is NOT an acceptable shipped
  assertion.

A pane is "ready" iff the tail of its `recent-unwrapped` buffer matches its
type's anchored prompt regex.

**Done when:** `hmux` launches a scratch team into herdr; every pane's
tail-window `pane read` matches its type's empirically-confirmed prompt regex.


===== ap-84f.1.5 =====
TITLE: hmux S-5: launch + spawn modes
# S-5 — launch + spawn modes

- `hmux launch <pane-name>`: **two-stage lookup** per the epic "Pane naming"
  ruling — (1) config lookup by `name == <pane-name>`; (2) runtime match by that
  pane's resolved identity (label). Reuse or create the pane in its window,
  re-run init+command via `build_pane_commands` (session-mode derivation).
- `hmux <type> <name>`: new pane in spare tab, set env, launch agent via
  `build_spawn_commands`, register member:
  `atm teams add-member <team> <name> --agent-type <name> --pane-id <id>` (matching rmux's full invocation incl. `--agent-type`).
- **`--parent-session-id` parity (claude spawn only):** `build_spawn_commands`
  reads `leadSessionId` from `~/.claude/teams/<team>/config.json` and appends
  `--parent-session-id <id>` when present (rmux:82-93,125-127).
- **spawn `--model` default resolution:** per the S-1 spawn-mode command table
  (single source; not restated here).

**Done when:** both modes work against a running team; `launch <pane-name>`
resolves correctly when `name != ATM_IDENTITY`; spawn registers the member AND
carries `--parent-session-id` when the team config provides it.


===== ap-84f.1.6 =====
TITLE: hmux S-6: Nudge (herdr backend)
# S-6 — Nudge (herdr backend)

`atm-nudge` herdr backend: resolve ATM_IDENTITY → pane id via
`herdr pane list --workspace <ATM_TEAM>` + label match, then
`herdr pane run <id> "<atm xml envelope>"`.

## Hook wiring (explicit, ordered)

- **Order:** (1) resolve `ATM_TEAM` first via the nudge script's OWN team
  resolution (payload team → `.atm.toml` walk → ambient env) — this is
  `atm-nudge-xml-1.py`'s existing `resolve_team()`, DISTINCT from
  `hmux_core.resolve_team` and outside the epic's launch-core ruling; (2)
  select backend: **herdr** iff `herdr` is on PATH AND
  `herdr pane list --workspace <ATM_TEAM>` succeeds; else **tmux** path
  unchanged.
- **Files changed:** `atm-nudge-xml-1.py` gains the herdr backend (selected by
  the ordered detection above). NO new parallel script; NO `.atm.toml`
  `[[atm.post_send_hooks]]` change — the existing hook entry stays.

## Envelope contract — TWO variants (pinned, verified atm-nudge-xml-1.py:216-258)

The envelope is selected by payload field `is_ack`:

**non-ack** (`is_ack` falsy):
```
<atm><action>read atm --team <team></action><action>ack the message</action><message-id><id></message-id><description><desc></description><action>execute the assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>
```

**ack** (`is_ack: true`):
```
<atm><action>read atm --team <team></action><action>ack the message</action><message-id><id></message-id><description><desc></description><action>message <id> acknowledged</action><action>complete associated work immediately</action><when idle="immediate" busy="complete tasks based on established priority"/><console announce="concise" pause="false"/></atm>
```

`<message-id>` and `<description>` tags are present only when the payload
carries them. Both variants are byte-identical to what tmux delivers today;
submitted via `pane run` (text + Enter), single-line XML.

**Done when:** `atm send <agent> "test"` delivers the non-ack envelope to the
correct pane and the agent acts; an ack nudge delivers the ack variant.


===== ap-84f.1.7 =====
TITLE: hmux S-7: Integration test (Luna, no real agent)
# S-7 — Integration test (Luna, no real agent)

Integration test against a fast low-cost agent (`Luna`) — NOT a real/expensive
agent.

**Luna defined concretely:**
- Identity: `ATM_IDENTITY = luna`, team = the scratch team under test.
- Launch: `hmux codex luna` (codex = fast/low-cost vs claude sonnet; uses
  `codex -c features.codex_hooks=true --yolo`).
- Registration: `atm teams add-member <team> luna --agent-type luna --pane-id <id>`.

**Test flow + round-trip assertion (NONCE, not prompt sentinel):**

The idle codex prompt `>` is present BEFORE and AFTER the nudge, so it cannot
prove a round-trip. Instead:

- Inject a **unique nonce** into the nudge envelope's `message-id` field
  (e.g. `rtt-<uuid4>`).
- Nudge via `herdr pane run <pane> "<atm xml envelope>"` (non-ack variant,
  carrying that nonce).
- Assert `pane read --source recent-unwrapped` contains the **nonce string**
  appearing AFTER the nudge was submitted.

**Ack-echo dependency (must be confirmed in S-3 first):** this assertion
assumes Luna's ack path emits output referencing the message-id to the pane.
S-3's live exercise MUST confirm whether plain `codex` echoes the message-id
before S-7 runs; if it does not, the nonce must instead be carried in a field
the agent demonstrably echoes (e.g. the description rendered into its turn
output). Record the observed echo behavior in S-3 and pin the assertion to it.

This distinguishes "Luna processed the envelope and referenced the message id"
from "Luna sat at `>` the whole time".

**Done when:** a scripted test launches a session, nudges Luna with a nonce,
and asserts the nonce appears in the pane-read output post-nudge — runnable
from a non-TTY shell.


