#!/usr/bin/env python3
"""S-2 — launch-core test suite (edge + corner cases).

Pins hmux_core against the ratified rules and rmux source. Run:
    pytest -q test_hmux_core.py
or (no pytest):
    python3 test_hmux_core.py

Shell dialects: bash output is pinned with explicit `shell="bash"` so it is
byte-for-byte deterministic on every platform; pwsh output is pinned with
`shell="pwsh"`. `shell=""` (auto) is tested only for the resolver itself.
"""

import os
import sys
import unittest
import tempfile
import textwrap
from unittest.mock import patch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hmux_core as h


def make_pane(**kw):
    defaults = dict(name="pane-0", dir="", model="", command="", env={},
                    agent="", prompt="")
    defaults.update(kw)
    return h.Pane(**defaults)


def make_cfg(session="default", windows=None, *, atm_default_team="",
             core_default_team="", env_dir="/tmp", env_file="/tmp/.env"):
    if windows is None:
        windows = []
    return h.RmuxConfig(
        session=session, windows=windows, env_dir=env_dir, env_file=env_file,
        atm_default_team=atm_default_team, core_default_team=core_default_team,
        session_default_team=atm_default_team,
    )


class TestIdentity(unittest.TestCase):

    def test_trio_identity_agent_field_wins(self):
        p = make_pane(name="secondary", agent="explicit-agent",
                      env={"ATM_IDENTITY": "env-identity", "ATM_TEAM": "t"})
        self.assertEqual(h.resolve_trio_identity(p), "explicit-agent")

    def test_trio_identity_env_fallback(self):
        p = make_pane(name="secondary", env={"ATM_IDENTITY": "env-identity"})
        self.assertEqual(h.resolve_trio_identity(p), "env-identity")

    def test_trio_identity_team_lead_skipped(self):
        p = make_pane(name="team-lead", env={"ATM_IDENTITY": "team-lead"})
        self.assertIsNone(h.resolve_trio_identity(p))

    def test_trio_identity_none(self):
        p = make_pane(name="pane-0")
        self.assertIsNone(h.resolve_trio_identity(p))

    def test_pane_label_precedence_env_wins(self):
        p = make_pane(name="cfg-name", agent="explicit-agent",
                      env={"ATM_IDENTITY": "env-identity"})
        self.assertEqual(h.resolve_pane_label(p), "env-identity")

    def test_pane_label_agent_then_name(self):
        p = make_pane(name="cfg-name", agent="explicit-agent")
        self.assertEqual(h.resolve_pane_label(p), "explicit-agent")

    def test_pane_label_name_fallback(self):
        p = make_pane(name="cfg-name")
        self.assertEqual(h.resolve_pane_label(p), "cfg-name")

    def test_pane_label_name_ne_identity(self):
        # name != ATM_IDENTITY -> label = ATM_IDENTITY (not name)
        p = make_pane(name="secondary", env={"ATM_IDENTITY": "alpha-prime"})
        self.assertEqual(h.resolve_pane_label(p), "alpha-prime")


class TestTeamResolution(unittest.TestCase):

    def test_spawn_team_cli_wins(self):
        cfg = make_cfg(core_default_team="core-team")
        self.assertEqual(h.resolve_team(cfg, "spawn", cli_team="cli-team"), "cli-team")

    def test_spawn_team_core_fallback(self):
        cfg = make_cfg(core_default_team="core-team")
        self.assertEqual(h.resolve_team(cfg, "spawn"), "core-team")

    def test_spawn_team_ambient_env(self):
        cfg = make_cfg()
        self.assertEqual(h.resolve_team(cfg, "spawn", env={"ATM_TEAM": "env-team"}), "env-team")

    def test_spawn_team_does_not_read_atm(self):
        # negative: spawn must NOT read [atm].default_team (rmux:56-62 uses [core])
        cfg = make_cfg(atm_default_team="atm-team")
        with self.assertRaises(h.TeamError):
            h.resolve_team(cfg, "spawn", env={})

    def test_spawn_team_missing_raises(self):
        cfg = make_cfg()
        with self.assertRaises(h.TeamError):
            h.resolve_team(cfg, "spawn", env={})

    def test_session_team_atm_default(self):
        cfg = make_cfg(atm_default_team="atm-team")
        self.assertEqual(h.resolve_team(cfg, "session"), "atm-team")

    def test_session_team_env_file_wins(self):
        cfg = make_cfg(atm_default_team="atm-team")
        cfg.session_default_team = "env-file-team"
        self.assertEqual(h.resolve_team(cfg, "session"), "env-file-team")


class TestShellResolution(unittest.TestCase):

    def test_explicit_wins(self):
        self.assertEqual(h.resolve_shell("pwsh"), "pwsh")
        self.assertEqual(h.resolve_shell("bash"), "bash")

    def test_auto_matches_platform(self):
        expected = "pwsh" if os.name == "nt" else "bash"
        self.assertEqual(h.resolve_shell(""), expected)

    def test_env_override_pwsh(self):
        with patch.dict(os.environ, {"HMUX_SHELL": "pwsh"}):
            self.assertEqual(h.resolve_shell(""), "pwsh")

    def test_env_override_bash(self):
        with patch.dict(os.environ, {"HMUX_SHELL": "bash"}):
            self.assertEqual(h.resolve_shell(""), "bash")

    def test_env_override_posix_alias(self):
        with patch.dict(os.environ, {"HMUX_SHELL": "sh"}):
            self.assertEqual(h.resolve_shell(""), "bash")

    def test_env_override_beaten_by_explicit(self):
        with patch.dict(os.environ, {"HMUX_SHELL": "pwsh"}):
            self.assertEqual(h.resolve_shell("bash"), "bash")


class TestQuote(unittest.TestCase):

    def test_pwsh_single_quotes_escape(self):
        self.assertEqual(h.quote("o'brien", "pwsh"), "'o''brien'")

    def test_pwsh_backslashes_literal(self):
        self.assertEqual(h.quote(r"C:\Users\rand\team dir", "pwsh"),
                         "'C:\\Users\\rand\\team dir'")

    def test_pwsh_always_quotes_safe_tokens(self):
        self.assertEqual(h.quote("agent-x@t", "pwsh"), "'agent-x@t'")

    def test_bash_delegates_to_shlex_safe_token(self):
        # shlex leaves @ and - unquoted -> byte-for-byte rmux output
        self.assertEqual(h.quote("agent-x@t", "bash"), "agent-x@t")

    def test_bash_quotes_spaces(self):
        self.assertEqual(h.quote("a b", "bash"), "'a b'")


class TestInitCommand(unittest.TestCase):

    def _cfg_dir(self):
        return tempfile.mkdtemp()

    def test_session_init_exports_all_env_keys(self):
        cfg_dir = self._cfg_dir()
        p = make_pane(dir="", env={"ATM_IDENTITY": "x", "ATM_TEAM": "t", "CUSTOM": "v"})
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="bash")
        self.assertIn("export ATM_IDENTITY=x", cmd)
        self.assertIn("export ATM_TEAM=t", cmd)
        self.assertIn("export CUSTOM=v", cmd)  # custom (non-ATM) key kept

    def test_session_init_no_unconditional_identity(self):
        cfg_dir = self._cfg_dir()
        p = make_pane(env={"CUSTOM": "v"})  # no ATM_* keys
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="bash")
        self.assertNotIn("ATM_IDENTITY", cmd)
        self.assertNotIn("ATM_TEAM", cmd)
        self.assertIn("export CUSTOM=v", cmd)

    def test_spawn_init_identity_and_team(self):
        cfg_dir = self._cfg_dir()
        p = make_pane(name="agent-x", env={"ATM_IDENTITY": "agent-x", "ATM_TEAM": "t"})
        cmd = h.build_init_cmd(p, cfg_dir, "spawn", shell="bash")
        self.assertIn("export ATM_IDENTITY=agent-x", cmd)
        self.assertIn("export ATM_TEAM=t", cmd)

    def test_spawn_init_no_extra_env(self):
        cfg_dir = self._cfg_dir()
        p = make_pane(name="agent-x", env={"ATM_IDENTITY": "agent-x", "ATM_TEAM": "t", "CUSTOM": "v"})
        cmd = h.build_init_cmd(p, cfg_dir, "spawn", shell="bash")
        self.assertNotIn("CUSTOM", cmd)  # spawn exports ONLY identity+team

    def test_env_file_source_when_present(self):
        cfg_dir = tempfile.mkdtemp()
        with open(os.path.join(cfg_dir, ".env"), "w") as f:
            f.write("ATM_TEAM=from-env-file\n")
        p = make_pane(env={})
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="bash")
        self.assertIn("set -a", cmd)
        self.assertIn("source", cmd)
        self.assertIn("set +a", cmd)

    def test_no_env_file_source_when_absent(self):
        cfg_dir = tempfile.mkdtemp()
        p = make_pane(env={})
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="bash")
        self.assertNotIn("source", cmd)

    def test_dir_default_vs_explicit(self):
        cfg_dir = tempfile.mkdtemp()
        # explicit dir (POSIX-safe) in bash
        p = make_pane(dir="/custom/dir", env={})
        self.assertIn("cd /custom/dir", h.build_init_cmd(p, cfg_dir, "session", shell="bash"))
        # default dir = cfg_dir; assert against the shell-quoted form so the
        # test is platform-independent
        p2 = make_pane(dir="", env={})
        self.assertIn(h.quote(cfg_dir, "bash"),
                      h.build_init_cmd(p2, cfg_dir, "session", shell="bash"))

    # ── pwsh ───────────────────────────────────────────────────────────

    def test_pwsh_spawn_init_identity_and_team(self):
        p = make_pane(name="agent-x", env={"ATM_IDENTITY": "agent-x", "ATM_TEAM": "t"})
        cmd = h.build_init_cmd(p, "/tmp", "spawn", shell="pwsh")
        self.assertIn("$env:ATM_IDENTITY = 'agent-x'", cmd)
        self.assertIn("$env:ATM_TEAM = 't'", cmd)
        self.assertNotIn("export ", cmd)

    def test_pwsh_session_init_exports_all_env_keys(self):
        p = make_pane(env={"ATM_IDENTITY": "x", "CUSTOM": "v"})
        cmd = h.build_init_cmd(p, "/tmp", "session", shell="pwsh")
        self.assertIn("$env:ATM_IDENTITY = 'x'", cmd)
        self.assertIn("$env:CUSTOM = 'v'", cmd)
        self.assertNotIn("export ", cmd)

    def test_pwsh_quotes_windows_path(self):
        p = make_pane(dir=r"C:\Users\rand\team dir", env={})
        cmd = h.build_init_cmd(p, "/tmp", "session", shell="pwsh")
        self.assertIn("cd 'C:\\Users\\rand\\team dir'", cmd)

    def test_pwsh_env_file_uses_get_content(self):
        cfg_dir = tempfile.mkdtemp()
        with open(os.path.join(cfg_dir, ".env"), "w") as f:
            f.write("ATM_TEAM=from-env-file\n")
        p = make_pane(env={})
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="pwsh")
        self.assertIn("Get-Content -LiteralPath", cmd)
        self.assertNotIn("set -a", cmd)
        self.assertNotIn("source", cmd)

    def test_pwsh_no_bash_constructs(self):
        cfg_dir = tempfile.mkdtemp()
        p = make_pane(env={"ATM_IDENTITY": "x"})
        cmd = h.build_init_cmd(p, cfg_dir, "session", shell="pwsh")
        self.assertNotIn("set -a", cmd)
        self.assertNotIn("hash -r", cmd)


class TestBuildPaneCommands(unittest.TestCase):

    def test_team_lead_no_trio(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "team-lead", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="bash")
        self.assertNotIn("--agent-id", cmd)
        self.assertNotIn("--agent-name", cmd)
        self.assertNotIn("--team-name", cmd)

    def test_secondary_gets_trio(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="bash")
        self.assertIn("--agent-id secondary@t", cmd)
        self.assertIn("--agent-name secondary", cmd)
        self.assertIn("--team-name t", cmd)

    def test_no_teammate_mode_ever(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="bash")
        self.assertNotIn("--teammate-mode", cmd)
        p2 = make_pane(model="sonnet", env={"ATM_IDENTITY": "team-lead"})
        self.assertNotIn("--teammate-mode", h.build_pane_commands(p2, "t", shell="bash"))

    def test_trio_uses_agent_field(self):
        p = make_pane(model="sonnet", agent="explicit",
                      env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="bash")
        self.assertIn("--agent-id explicit@t", cmd)
        self.assertIn("--agent-name explicit", cmd)

    def test_trio_team_falls_back_to_default(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary"})  # no ATM_TEAM
        cmd = h.build_pane_commands(p, "default-team", shell="bash")
        self.assertIn("--agent-id secondary@default-team", cmd)
        self.assertIn("--team-name default-team", cmd)

    def test_trio_team_uses_pane_env_first(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "pane-team"})
        cmd = h.build_pane_commands(p, "default-team", shell="bash")
        self.assertIn("--agent-id secondary@pane-team", cmd)
        self.assertIn("--team-name pane-team", cmd)

    def test_raw_command_passthrough(self):
        p = make_pane(command="codex -c features.hooks=true --yolo")
        self.assertEqual(h.build_pane_commands(p, "t"), "codex -c features.hooks=true --yolo")

    def test_raw_command_with_prompt(self):
        p = make_pane(command="gemini", prompt="do the thing")
        self.assertEqual(h.build_pane_commands(p, "t"), "gemini do the thing")

    def test_model_command_with_prompt(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "t"},
                      prompt="do the thing")
        cmd = h.build_pane_commands(p, "t", shell="bash")
        self.assertTrue(cmd.endswith(" do the thing"))

    def test_missing_team_trio_still_builds_with_default(self):
        # build_pane_commands always gets a resolved team; empty team just yields
        # an empty trio team (caller's responsibility is resolve_team).
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary"})
        cmd = h.build_pane_commands(p, "", shell="bash")
        self.assertIn("--agent-id secondary@", cmd)

    # ── pwsh ───────────────────────────────────────────────────────────

    def test_pwsh_secondary_gets_quoted_trio(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "secondary", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="pwsh")
        self.assertIn("--model 'sonnet'", cmd)
        self.assertIn("--agent-id 'secondary@t'", cmd)
        self.assertIn("--agent-name 'secondary'", cmd)
        self.assertIn("--team-name 't'", cmd)
        self.assertNotIn("--teammate-mode", cmd)

    def test_pwsh_team_lead_still_no_trio(self):
        p = make_pane(model="sonnet", env={"ATM_IDENTITY": "team-lead", "ATM_TEAM": "t"})
        cmd = h.build_pane_commands(p, "t", shell="pwsh")
        self.assertNotIn("--agent-id", cmd)


class TestSpawnCommands(unittest.TestCase):

    def test_codex(self):
        self.assertEqual(h.build_spawn_commands("t", "codex", "n"),
                         "codex -c features.codex_hooks=true --yolo")

    def test_gemini(self):
        self.assertEqual(h.build_spawn_commands("t", "gemini", "n"), "gemini")

    def test_hermes_no_model(self):
        cmd = h.build_spawn_commands("t", "hermes", "agent-x", shell="bash")
        self.assertIn("--profile agent-x", cmd)
        self.assertNotIn(" -m ", cmd)

    def test_hermes_with_model(self):
        cmd = h.build_spawn_commands("t", "hermes", "agent-x", model="v4", shell="bash")
        self.assertIn("-m v4", cmd)

    def test_claude_default_model_sonnet(self):
        cmd = h.build_spawn_commands("t", "claude", "agent-x", shell="bash")
        self.assertIn("--model sonnet", cmd)

    def test_haiku_default_model(self):
        cmd = h.build_spawn_commands("t", "haiku", "agent-x", shell="bash")
        self.assertIn("--model haiku", cmd)

    def test_fable_default_model(self):
        cmd = h.build_spawn_commands("t", "fable", "agent-x", shell="bash")
        self.assertIn("--model fable", cmd)

    def test_claude_trio_always_present(self):
        cmd = h.build_spawn_commands("t", "sonnet", "agent-x", shell="bash")
        self.assertIn("--agent-id agent-x@t", cmd)
        self.assertIn("--agent-name agent-x", cmd)
        self.assertIn("--team-name t", cmd)
        self.assertNotIn("--teammate-mode", cmd)

    def test_parent_session_id(self):
        cmd = h.build_spawn_commands("t", "sonnet", "agent-x", parent_session_id="lead123", shell="bash")
        self.assertIn("--parent-session-id lead123", cmd)

    def test_no_parent_session_id(self):
        cmd = h.build_spawn_commands("t", "sonnet", "agent-x", shell="bash")
        self.assertNotIn("--parent-session-id", cmd)

    # ── pwsh ───────────────────────────────────────────────────────────

    def test_pwsh_claude_quoted_trio(self):
        cmd = h.build_spawn_commands("t", "sonnet", "agent-x", shell="pwsh")
        self.assertIn("--model 'sonnet'", cmd)
        self.assertIn("--agent-id 'agent-x@t'", cmd)
        self.assertIn("--agent-name 'agent-x'", cmd)
        self.assertIn("--team-name 't'", cmd)
        self.assertNotIn("--teammate-mode", cmd)

    def test_pwsh_hermes_quoted_profile(self):
        cmd = h.build_spawn_commands("t", "hermes", "agent-x", shell="pwsh")
        self.assertIn("--profile 'agent-x'", cmd)


class TestLoadConfig(unittest.TestCase):

    def _write_config(self, content):
        d = tempfile.mkdtemp()
        path = os.path.join(d, ".atm.toml")
        with open(path, "w") as f:
            f.write(content)
        return d, path

    def test_missing_file_raises(self):
        with self.assertRaises(h.ConfigError):
            h.load_config("/nonexistent/.atm.toml")

    def test_no_rmux_section_raises(self):
        d, path = self._write_config("[atm]\ndefault_team = \"t\"\n")
        with self.assertRaises(h.ConfigError):
            h.load_config(path)

    def test_parse_full_config(self):
        d, path = self._write_config(textwrap.dedent("""
            [atm]
            default_team = "atm-team"
            [core]
            default_team = "core-team"
            [rmux]
            session = "atm-team"
            [[rmux.windows]]
            name = "agents"
            layout = "even-horizontal"
            [[rmux.windows.panes]]
            name = "team-lead"
            model = "sonnet"
            env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "atm-team" }
            [[rmux.windows.panes]]
            name = "arch-ctm"
            command = "codex -c features.hooks=true --yolo"
            env = { ATM_IDENTITY = "arch-ctm", ATM_TEAM = "atm-team" }
        """))
        cfg = h.load_config(path)
        self.assertEqual(cfg.session, "atm-team")
        self.assertEqual(cfg.atm_default_team, "atm-team")
        self.assertEqual(cfg.core_default_team, "core-team")
        self.assertEqual(len(cfg.windows), 1)
        self.assertEqual(len(cfg.windows[0].panes), 2)
        self.assertEqual(cfg.windows[0].panes[0].name, "team-lead")
        self.assertEqual(cfg.windows[0].panes[1].command, "codex -c features.hooks=true --yolo")

    def test_empty_windows(self):
        d, path = self._write_config("[rmux]\nsession = \"x\"\n")
        cfg = h.load_config(path)
        self.assertEqual(cfg.windows, [])


class TestDryRunProjection(unittest.TestCase):

    def test_projection_contains_panes(self):
        pane = make_pane(name="team-lead", model="sonnet",
                         env={"ATM_IDENTITY": "team-lead", "ATM_TEAM": "t"})
        cfg = make_cfg(session="t", windows=[h.Window(name="agents", layout="tiled",
                                                      panes=[pane])])
        out = h.dry_run_projection(cfg, "t", mode="session", shell="bash")
        self.assertIn('Pane: "team-lead"', out)
        self.assertIn("role: primary (team-lead, no agent trio)", out)
        self.assertNotIn("--teammate-mode", out)

    def test_projection_session_key_is_team(self):
        cfg = make_cfg(session="different-session", windows=[])
        out = h.dry_run_projection(cfg, "atm-team", mode="session", shell="bash")
        self.assertIn("=== hmux dry-run: atm-team ===", out)
        self.assertNotIn("different-session", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
