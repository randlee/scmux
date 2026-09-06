#!/usr/bin/env python3
"""hmux_core — shared launch-core for hmux (herdr-native ATM team launcher).

Faithful extraction of `scripts/rmux`'s config parsing + agent-command
derivation, with the tmux backend removed. This is the single import point
for hmux (and, later, rmux + scmux-daemon).

Rules duplicated here mirror rmux source exactly, except the ratified deltas
(see the hmux plan, beads ap-84f):
  * claude commands DROP `--teammate-mode tmux` (tmux integration won't work
    under herdr); the undocumented trio `--agent-id/--agent-name/--team-name`
    is KEPT.
  * two distinct identity concepts:
      - resolve_trio_identity()  -> claude trio name (agent field first,
        then env.ATM_IDENTITY non-empty and != team-lead, else None).
      - resolve_pane_label()     -> herdr pane label (env.ATM_IDENTITY ->
        agent -> name), always non-empty.
  * team resolution is mode-specific (spawn [core] vs session .env/[atm]).

No herdr/tmux dependency. Pure functions only.
"""

from __future__ import annotations

import os
import shlex
import shutil
from dataclasses import dataclass, field
from typing import Dict, List, Mapping, Optional

__all__ = [
    "ConfigError",
    "TeamError",
    "Pane",
    "Window",
    "RmuxConfig",
    "SHELL_BASH",
    "SHELL_PWSH",
    "resolve_shell",
    "quote",
    "load_config",
    "resolve_team",
    "resolve_trio_identity",
    "resolve_pane_label",
    "build_init_cmd",
    "build_pane_commands",
    "build_spawn_commands",
    "dry_run_projection",
    "resolve_binary",
    "env_file_path",
]

# Spawn default-model resolution (rmux:73-79). Faithful: codex|gemini|hermes
# have NO default model; claude-family maps claude|sonnet->sonnet, haiku->haiku,
# opus->opus, fable->fable.
DEFAULT_SPAWN_MODEL = {
    "claude": "sonnet",
    "sonnet": "sonnet",
    "haiku": "haiku",
    "opus": "opus",
    "fable": "fable",
}

# Shell dialects hmux emits. bash = POSIX sh (original rmux output, byte-for-byte
# unchanged). pwsh = PowerShell, emitted on Windows where herdr panes run
# PowerShell. Pure functions accept an explicit `shell`; empty/unknown values
# resolve via HMUX_SHELL env, then `os.name` (nt -> pwsh, else bash).
SHELL_BASH = "bash"
SHELL_PWSH = "pwsh"


def resolve_shell(shell: str = "") -> str:
    """Resolve the target shell dialect. Explicit value wins; then HMUX_SHELL
    env; then platform (`os.name == "nt"` -> pwsh, else bash)."""
    if shell in (SHELL_BASH, SHELL_PWSH):
        return shell
    override = os.environ.get("HMUX_SHELL", "").strip().lower()
    if override in ("bash", "posix", "sh"):
        return SHELL_BASH
    if override in ("pwsh", "powershell"):
        return SHELL_PWSH
    return SHELL_PWSH if os.name == "nt" else SHELL_BASH


def quote(value: str, shell: str = "") -> str:
    """Quote a single argument for the target shell.

    bash -> shlex.quote (POSIX). pwsh -> single-quote with '' escaping, which
    treats backslashes literally (correct for Windows paths, unlike POSIX).
    """
    shell = resolve_shell(shell)
    if shell == SHELL_PWSH:
        return "'" + value.replace("'", "''") + "'"
    return shlex.quote(value)


def _pwsh_source_env(env_file: str) -> str:
    """PowerShell equivalent of bash's `set -a; source .env; set +a`: load
    KEY=VALUE lines (skipping blanks/comments, stripping surrounding quotes)
    into the process environment. Verified live against herdr 0.8.2 panes."""
    qf = quote(env_file, SHELL_PWSH)
    return (
        f"Get-Content -LiteralPath {qf} -ErrorAction SilentlyContinue "
        "| ForEach-Object { if ($_ -match '^\\s*([^#=\\s]+)\\s*=\\s*(.*)$') { "
        "$n = $matches[1]; $v = $matches[2].Trim(); "
        "$v = $v.Trim([char]34).Trim([char]39); "
        "Set-Item -Path (\"Env:\" + $n) -Value $v } }"
    )


class ConfigError(Exception):
    """Config missing, malformed, or has no [rmux] section."""


class TeamError(Exception):
    """ATM_TEAM could not be resolved for the requested mode."""


@dataclass
class Pane:
    name: str = "pane-0"
    dir: str = ""                      # working dir (defaults to config dir)
    model: str = ""                    # claude model (=> claude command)
    command: str = ""                  # raw command (codex/gemini/hermes/shell)
    env: Dict[str, str] = field(default_factory=dict)
    agent: str = ""                    # explicit agent field
    prompt: str = ""                   # trailing prompt appended to command


@dataclass
class Window:
    name: str = "win-0"
    layout: str = "tiled"
    panes: List[Pane] = field(default_factory=list)


@dataclass
class RmuxConfig:
    session: str = "default"           # [rmux].session (READ but IGNORED by hmux)
    windows: List[Window] = field(default_factory=list)
    env_dir: str = ""                  # dir containing the config file
    env_file: str = ""                 # env_dir/.env
    atm_default_team: str = ""         # [atm].default_team
    core_default_team: str = ""        # [core].default_team
    session_default_team: str = ""     # .env ATM_TEAM= or [atm].default_team


def env_file_path(config_dir: str) -> str:
    """Return config_dir/.env."""
    return os.path.join(config_dir, ".env")


def resolve_binary(name: str) -> str:
    """`whence -p <name>` equivalent: absolute path, or the bare name."""
    return shutil.which(name) or name


def _extract_env_team(env_file: str) -> str:
    """Extract `ATM_TEAM=<value>` from a dotenv file (first match, quotes stripped).

    Mirrors rmux:309 (`grep '^ATM_TEAM=' | head -1 | cut -d= -f2- | tr -d quotes`).
    """
    try:
        with open(env_file, "r") as f:
            for line in f:
                line = line.strip()
                if line.startswith("ATM_TEAM="):
                    val = line.split("=", 1)[1].strip()
                    return val.strip("'\"")
    except OSError:
        pass
    return ""


def load_config(config_path: str) -> RmuxConfig:
    """Parse .atm.toml into an RmuxConfig. Raises ConfigError on failure."""
    try:
        import tomllib
    except ImportError:  # Python < 3.11
        try:
            import tomli as tomllib
        except ImportError:
            raise ConfigError("neither tomllib nor tomli available; need Python 3.11+ or `pip install tomli`")

    config_path = os.path.abspath(config_path)
    if not os.path.isfile(config_path):
        raise ConfigError(f"config not found: {config_path}")

    with open(config_path, "rb") as f:
        data = tomllib.load(f)

    if "rmux" not in data:
        raise ConfigError("no [rmux] section in config")

    rmux = data["rmux"]
    env_dir = os.path.dirname(config_path)
    env_file = env_file_path(env_dir)
    env_team = _extract_env_team(env_file) if os.path.isfile(env_file) else ""

    windows: List[Window] = []
    for wi, w in enumerate(rmux.get("windows", [])):
        panes: List[Pane] = []
        for pi, p in enumerate(w.get("panes", [])):
            panes.append(Pane(
                name=p.get("name", f"pane-{pi}"),
                dir=p.get("dir", ""),
                model=p.get("model", ""),
                command=p.get("command", ""),
                env=dict(p.get("env", {})),
                agent=p.get("agent", ""),
                prompt=p.get("prompt", ""),
            ))
        windows.append(Window(
            name=w.get("name", f"win-{wi}"),
            layout=w.get("layout", "tiled"),
            panes=panes,
        ))

    atm_default = data.get("atm", {}).get("default_team", "")
    core_default = data.get("core", {}).get("default_team", "")

    return RmuxConfig(
        session=rmux.get("session", "default"),
        windows=windows,
        env_dir=env_dir,
        env_file=env_file,
        atm_default_team=atm_default,
        core_default_team=core_default,
        session_default_team=env_team or atm_default,
    )


def resolve_team(cfg: RmuxConfig, mode: str, cli_team: Optional[str] = None,
                 env: Optional[Mapping[str, str]] = None) -> str:
    """Resolve the default ATM_TEAM for a mode. Raises TeamError if empty.

    Mode-specific, faithful to rmux:
      * spawn   -> cli_team -> [core].default_team -> ambient ATM_TEAM env
      * session -> .env ATM_TEAM= -> [atm].default_team
    """
    env = env if env is not None else os.environ
    if mode == "spawn":
        team = cli_team or cfg.core_default_team or env.get("ATM_TEAM", "")
    elif mode == "session":
        team = cfg.session_default_team
    else:
        raise ValueError(f"unknown mode: {mode!r}")

    if not team:
        raise TeamError(
            "cannot determine ATM_TEAM — "
            + ("set [core] default_team in .atm.toml or pass --team"
               if mode == "spawn"
               else "set ATM_TEAM in .env or [atm] default_team in .atm.toml")
        )
    return team


def resolve_trio_identity(pane: Pane) -> Optional[str]:
    """Claude trio agent name. Faithful to rmux:377-384.

    explicit `agent` field -> env.ATM_IDENTITY (non-empty, != team-lead) -> None.
    """
    if pane.agent:
        return pane.agent
    identity = pane.env.get("ATM_IDENTITY", "")
    if identity and identity != "team-lead":
        return identity
    return None


def resolve_pane_label(pane: Pane) -> str:
    """Herdr pane label. Always non-empty: env.ATM_IDENTITY -> agent -> name."""
    identity = pane.env.get("ATM_IDENTITY", "")
    if identity:
        return identity
    if pane.agent:
        return pane.agent
    return pane.name


def build_init_cmd(pane: Pane, config_dir: str, mode: str, shell: str = "") -> str:
    """Build the init command for a pane.

    mode "spawn"   (rmux:99-103): cd <dir>; [load .env]; export ATM_IDENTITY/
                                  ATM_TEAM (from the synthetic pane's env/name).
    mode "session" (rmux:326-348): cd <dir>; [load .env]; export EVERY key in
                                   pane.env (ATM_* only when present).

    `shell` selects the dialect: "bash" (POSIX sh, byte-for-byte rmux) or
    "pwsh" (PowerShell, for Windows herdr panes). Empty resolves via
    HMUX_SHELL then platform.
    """
    shell = resolve_shell(shell)
    q = lambda v: quote(v, shell)  # noqa: E731
    work_dir = pane.dir or config_dir
    parts = [f"cd {q(work_dir)}"]

    env_file = env_file_path(config_dir)
    if os.path.isfile(env_file):
        if shell == SHELL_PWSH:
            parts.append(_pwsh_source_env(env_file))
        else:
            parts += ["set -a", f"source {q(env_file)}", "set +a", "hash -r"]

    if mode == "spawn":
        identity = pane.env.get("ATM_IDENTITY") or pane.name
        team = pane.env.get("ATM_TEAM", "")
        if shell == SHELL_PWSH:
            parts.append(f"$env:ATM_IDENTITY = {q(identity)}")
            if team:
                parts.append(f"$env:ATM_TEAM = {q(team)}")
        else:
            parts.append(f"export ATM_IDENTITY={q(identity)}")
            if team:
                parts.append(f"export ATM_TEAM={q(team)}")
    else:  # session
        for k, v in pane.env.items():
            if shell == SHELL_PWSH:
                parts.append(f"$env:{k} = {q(str(v))}")
            else:
                parts.append(f"export {k}={q(str(v))}")

    return "; ".join(parts)


def build_pane_commands(pane: Pane, team: str, shell: str = "") -> str:
    """Build the agent full command for a config pane (session + launch modes).

    `team` is the session default team (from resolve_team(cfg, "session")).
    The trio team is pane.env.ATM_TEAM, falling back to `team` (rmux:387-388).

    model pane -> claude command (trio for secondary, absent for team-lead,
                 `--teammate-mode tmux` DROPPED per ruling).
    raw pane   -> passthrough of pane.command (codex/gemini/hermes/shell).

    `shell` selects argument quoting: "bash" (shlex/POSIX) or "pwsh" (single
    quotes). Empty resolves via HMUX_SHELL then platform.
    """
    shell = resolve_shell(shell)
    q = lambda v: quote(v, shell)  # noqa: E731
    if pane.model:
        cmd = f"{resolve_binary('claude')} --model {q(pane.model)} --dangerously-skip-permissions"
        agent_name = resolve_trio_identity(pane)
        if agent_name:
            atm_team = pane.env.get("ATM_TEAM", "") or team
            cmd += (
                f" --agent-id {q(f'{agent_name}@{atm_team}')}"
                f" --agent-name {q(agent_name)}"
                f" --team-name {q(atm_team)}"
            )
        if pane.prompt:
            cmd += f" {pane.prompt}"
        return cmd

    cmd = pane.command or ""
    if pane.prompt:
        cmd = f"{cmd} {pane.prompt}".strip()
    return cmd


def build_spawn_commands(team: str, spawn_type: str, spawn_name: str,
                         model: Optional[str] = None,
                         parent_session_id: Optional[str] = None,
                         shell: str = "") -> str:
    """Build the agent full command for an ad-hoc spawn (rmux:105-128).

    codex/gemini/hermes/claude-family, with spawn default-model resolution and
    the claude trio (always present for spawn) + optional --parent-session-id.
    `--teammate-mode tmux` DROPPED per ruling.

    `shell` selects argument quoting ("bash" shlex vs "pwsh" single quotes);
    empty resolves via HMUX_SHELL then platform.
    """
    shell = resolve_shell(shell)
    q = lambda v: quote(v, shell)  # noqa: E731
    if spawn_type == "codex":
        return "codex -c features.codex_hooks=true --yolo"
    if spawn_type == "gemini":
        return "gemini"
    if spawn_type == "hermes":
        cmd = f"{resolve_binary('hermes')} --profile {q(spawn_name)}"
        if model:
            cmd += f" -m {q(model)}"
        return cmd

    # claude | sonnet | haiku | opus | fable
    resolved_model = model or DEFAULT_SPAWN_MODEL.get(spawn_type, "sonnet")
    cmd = f"{resolve_binary('claude')} --model {q(resolved_model)} --dangerously-skip-permissions"
    cmd += (
        f" --agent-id {q(f'{spawn_name}@{team}')}"
        f" --agent-name {q(spawn_name)}"
        f" --team-name {q(team)}"
    )
    if parent_session_id:
        cmd += f" --parent-session-id {q(parent_session_id)}"
    return cmd


def _dry_run_agent_line(pane: Pane) -> Optional[str]:
    """Mirror print_pane_dry_run's agent/role line (rmux:441-451)."""
    if pane.agent:
        return f"agent: {pane.agent} (explicit)"
    if pane.model:
        identity = pane.env.get("ATM_IDENTITY", "")
        if identity and identity != "team-lead":
            return f"agent: {identity} (from ATM_IDENTITY)"
        if identity == "team-lead":
            return "role: primary (team-lead, no agent trio)"
    return None


def dry_run_projection(cfg: RmuxConfig, team: str, *, mode: str = "session",
                       shell: str = "") -> str:
    """Render a dry-run projection matching rmux --dry-run, modulo the
    ratified deltas (no --teammate-mode tmux; workspace key = ATM_TEAM).

    Only mode="session" (full launch) is implemented in S-1; spawn/launch
    projections are added with their respective sprints.

    `shell` selects the init/command dialect ("bash" or "pwsh"); empty
    resolves via HMUX_SHELL then platform.
    """
    if mode != "session":
        raise NotImplementedError(f"dry_run_projection mode={mode!r} not implemented in S-1")

    lines: List[str] = []
    lines.append(f"=== hmux dry-run: {team} ===")
    lines.append("")
    if os.path.isfile(cfg.env_file):
        lines.append(f"Repo env file: {cfg.env_file}")
    else:
        lines.append(f"Repo env file missing: {cfg.env_file}")
    lines.append("")

    for wi, window in enumerate(cfg.windows):
        if not window.panes:
            continue
        lines.append(f'Window {wi}: "{window.name}" (layout: {window.layout})')
        for pane in window.panes:
            lines.append(f'  Pane: "{pane.name}"')
            if pane.dir:
                lines.append(f"    dir: {pane.dir}")
            init = build_init_cmd(pane, cfg.env_dir, "session", shell=shell)
            if init:
                lines.append(f"    init: {init}")
            if pane.model:
                lines.append(f"    model: {pane.model}")
            agent_line = _dry_run_agent_line(pane)
            if agent_line:
                lines.append(f"    {agent_line}")
            if pane.prompt:
                lines.append(f"    prompt: {pane.prompt}")
            full = build_pane_commands(pane, team, shell=shell)
            if full:
                lines.append(f"    cmd: {full}")
        lines.append("")

    lines.append("Run without --dry-run to create session.")
    return "\n".join(lines).rstrip("\n") + "\n"
