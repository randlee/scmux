#!/usr/bin/env python3
"""Unit tests for hmux's Herdr registration lifecycle."""

import contextlib
import importlib.util
import io
import os
import subprocess
import sys
import unittest
from importlib.machinery import SourceFileLoader
from types import SimpleNamespace
from unittest.mock import patch

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPTS_DIR)
import hmux_core as core  # noqa: E402
import hmux_backend as backend  # noqa: E402

_LOADER = SourceFileLoader("hmux_launcher", os.path.join(SCRIPTS_DIR, "hmux"))
_SPEC = importlib.util.spec_from_loader("hmux_launcher", _LOADER)
hmux = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
_SPEC.loader.exec_module(hmux)


def make_cfg(pane):
    return core.RmuxConfig(
        windows=[core.Window(name="agents", panes=[pane])],
        env_dir="/tmp",
        env_file="/tmp/.env",
        session_default_team="team",
        core_default_team="team",
    )


class TestHerdrRegistration(unittest.TestCase):
    def test_session_registration_orders_rename_run_wait_rename_confirm(self):
        pane = core.Pane(
            name="worker",
            env={"ATM_IDENTITY": "worker", "ATM_TEAM": "team"},
            command="agent --start",
        )
        events = []
        responses = iter([
            None,
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])

        def record(name, result=None):
            def call(*args):
                events.append((name, *args))
                return result
            return call

        def agent_get(target):
            events.append(("agent_get", target))
            return next(responses)

        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(backend, "pane_find_by_label", return_value=None), \
             patch.object(backend, "pane_rename", side_effect=record("pane_rename")), \
             patch.object(backend, "pane_run", side_effect=record("pane_run")), \
             patch.object(hmux, "_resolve_herdr_target", return_value="worker"), \
             patch.object(backend, "agent_get", side_effect=agent_get), \
             patch.object(backend, "agent_rename", side_effect=record("agent_rename")), \
             patch.object(hmux.time, "sleep", side_effect=record("sleep")):
            result = hmux.run_session(make_cfg(pane), "team", False, shell="bash")

        self.assertEqual(result, 0)
        self.assertEqual(
            [event[0] for event in events],
            ["pane_rename", "pane_run", "agent_get", "sleep", "agent_get", "agent_rename", "agent_get"],
        )
        self.assertEqual(events[2][1], "pane")
        self.assertEqual(events[5][1:], ("pane", "worker"))
        self.assertEqual(events[6][1], "worker")

    def test_launch_registration_uses_alias_target(self):
        pane = core.Pane(
            name="worker",
            alias="ops",
            env={"ATM_IDENTITY": "worker", "ATM_TEAM": "team"},
            command="agent --start",
        )
        responses = iter([
            {"result": {"agent": {"pane_id": "other"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])
        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(hmux, "_resolve_herdr_target", return_value="ops"), \
             patch.object(backend, "pane_find_by_label", return_value="pane"), \
             patch.object(backend, "pane_run") as pane_run, \
             patch.object(backend, "agent_get", side_effect=lambda target: next(responses)) as get, \
             patch.object(backend, "agent_rename") as rename:
            result = hmux.run_launch(make_cfg(pane), "team", "worker", False, shell="bash")

        self.assertEqual(result, 0)
        pane_run.assert_not_called()
        rename.assert_called_once_with("pane", "ops")
        self.assertEqual([call.args[0] for call in get.call_args_list], ["ops", "pane", "ops"])

    def test_session_existing_pane_repairs_without_rerunning_agent(self):
        pane = core.Pane(
            name="worker",
            alias="ops",
            env={"ATM_IDENTITY": "worker", "ATM_TEAM": "team"},
            command="agent --start",
        )
        responses = iter([
            {"result": {"agent": {"pane_id": "other"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])
        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(hmux, "_resolve_herdr_target", return_value="ops"), \
             patch.object(backend, "pane_find_by_label", return_value="pane"), \
             patch.object(backend, "pane_run") as pane_run, \
             patch.object(backend, "agent_get", side_effect=lambda target: next(responses)), \
             patch.object(backend, "agent_rename") as rename:
            result = hmux.run_session(make_cfg(pane), "team", False, shell="bash")

        self.assertEqual(result, 0)
        pane_run.assert_not_called()
        rename.assert_called_once_with("pane", "ops")

    def test_registration_timeout_warns_and_returns_failure(self):
        pane = core.Pane(name="worker", command="agent --start")
        stderr = io.StringIO()
        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(backend, "pane_find_by_label", return_value=None), \
             patch.object(hmux, "_resolve_herdr_target", return_value="worker"), \
             patch.object(backend, "pane_rename"), \
             patch.object(backend, "pane_run"), \
             patch.object(backend, "agent_get", return_value=None) as get, \
             patch.object(hmux.time, "sleep"), \
             contextlib.redirect_stderr(stderr):
            result = hmux.run_session(make_cfg(pane), "team", False, shell="bash")

        self.assertEqual(result, 1)
        self.assertEqual(get.call_count, 30)
        self.assertIn("WARN: timed out waiting for Herdr agent in pane pane", stderr.getvalue())
        self.assertIn("herdr agent rename pane worker", stderr.getvalue())

    def test_spawn_registration_uses_herdr_backend_and_reports_add_member_error(self):
        cfg = make_cfg(core.Pane(name="spawned", alias="spawn-alias"))
        calls = iter([
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])
        fake_run = SimpleNamespace(returncode=1, stdout="", stderr="add-member failed")
        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(backend, "tab_list", return_value=[]), \
             patch.object(backend, "tab_create", return_value={"tab_id": "tab", "root_pane_id": "root"}), \
             patch.object(backend, "pane_split", return_value="pane"), \
             patch.object(backend, "pane_rename"), \
             patch.object(backend, "pane_run"), \
             patch.object(backend, "agent_get", side_effect=lambda target: next(calls)), \
             patch.object(backend, "agent_rename"), \
             patch.object(hmux.time, "sleep"), \
             patch.object(subprocess, "run", return_value=fake_run) as run:
            result = hmux.run_spawn(cfg, "codex", "spawned", "", "", "spare", False, shell="bash")

        self.assertEqual(result, 1)
        argv = run.call_args.args[0]
        self.assertEqual(
            argv,
            ["atm", "teams", "add-member", "team", "spawned", "--agent-type", "spawned",
             "--backend", "herdr", "--alias", "spawn-alias"],
        )
        self.assertNotIn("--pane-id", argv)

    def test_rename_agents_reports_already_correct_renamed_and_not_started(self):
        panes = [
            {"pane_id": "p1", "label": "lead"},
            {"pane_id": "p2", "label": "worker"},
            {"pane_id": "p3", "label": ""},
            {"pane_id": "p4", "label": "starting"},
        ]
        responses = iter([
            {"result": {"agent": {"pane_id": "p1"}}},
            None,
            {"result": {"agent": {"pane_id": "p2"}}},
            {"result": {"agent": {"pane_id": "p2"}}},
            None,
            None,
        ])
        stdout = io.StringIO()
        with patch.object(backend, "workspace_find_by_label", return_value="workspace"), \
             patch.object(backend, "pane_list", return_value=panes), \
             patch.object(backend, "agent_get", side_effect=lambda target: next(responses)), \
             patch.object(backend, "agent_rename") as rename, \
             patch.object(hmux, "_resolve_herdr_target", side_effect=lambda team, identity, config_alias="": identity), \
             contextlib.redirect_stdout(stdout):
            result = hmux.rename_agents(make_cfg(core.Pane(name="other")), "team")

        self.assertEqual(result, 0)
        rename.assert_called_once_with("p2", "worker")
        self.assertEqual(
            stdout.getvalue().splitlines(),
            [
                "already correct herdr agent 'lead' for pane p1",
                "renamed herdr agent 'worker' for pane p2",
                "no agent yet for pane p4",
            ],
        )

    def test_roster_top_level_alias_is_authoritative_and_config_mismatch_is_error(self):
        fake = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","alias":"roster-ops","extra":{}}]}',
            stderr="",
        )
        stderr = io.StringIO()
        with patch.object(hmux.subprocess, "run", return_value=fake) as run, \
             contextlib.redirect_stderr(stderr):
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "roster-ops")
        self.assertEqual(run.call_args.args[0], ["atm", "members", "--team", "team", "--json"])
        self.assertIn("config alias 'config-ops'", stderr.getvalue())
        self.assertIn("roster alias 'roster-ops'", stderr.getvalue())

    def test_legacy_extra_alias_is_used_when_top_level_alias_is_absent(self):
        members = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","extra":{"alias":"legacy-ops"}}]}',
            stderr="",
        )
        with patch.object(hmux.subprocess, "run", return_value=members):
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "legacy-ops")

    def test_top_level_null_alias_updates_once_and_uses_config_alias(self):
        members = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","alias":null,"extra":{}}]}',
            stderr="",
        )
        update = SimpleNamespace(returncode=0, stdout="", stderr="")
        with patch.object(hmux.subprocess, "run", side_effect=[members, update]) as run:
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "config-ops")
        self.assertEqual(run.call_args_list[1].args[0], [
            "atm", "teams", "update-member", "team", "worker", "--alias", "config-ops",
        ])

    def test_update_member_mutation_runs_as_target_team_member(self):
        members = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","alias":null,"extra":{}}]}',
            stderr="",
        )
        update = SimpleNamespace(returncode=0, stdout="", stderr="")
        with patch.object(hmux.subprocess, "run", side_effect=[members, update]) as run:
            target = hmux._resolve_herdr_target("atm-dev", "worker", "config-ops")

        self.assertEqual(target, "config-ops")
        kwargs = run.call_args_list[1].kwargs
        self.assertEqual(kwargs["env"]["ATM_TEAM"], "atm-dev")
        self.assertEqual(kwargs["env"]["ATM_IDENTITY"], "worker")

    def test_rename_agents_uses_config_alias_for_roster_repair(self):
        cfg = make_cfg(core.Pane(name="worker", alias="config-ops"))
        panes = [{"pane_id": "pane", "label": "worker"}]
        members = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","alias":null}]}',
            stderr="",
        )
        update = SimpleNamespace(returncode=0, stdout="", stderr="")
        responses = iter([
            None,
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])
        with patch.object(backend, "workspace_find_by_label", return_value="workspace"), \
             patch.object(backend, "pane_list", return_value=panes), \
             patch.object(backend, "agent_get", side_effect=lambda target: next(responses)), \
             patch.object(backend, "agent_rename") as rename, \
             patch.object(hmux.subprocess, "run", side_effect=[members, update]) as run:
            result = hmux.rename_agents(cfg, "team")

        self.assertEqual(result, 0)
        rename.assert_called_once_with("pane", "config-ops")
        self.assertEqual(run.call_args_list[1].args[0], [
            "atm", "teams", "update-member", "team", "worker", "--alias", "config-ops",
        ])

    def test_launch_repairs_existing_pane_without_command(self):
        pane = core.Pane(name="worker", command="")
        responses = iter([
            {"result": {"agent": {"pane_id": "other"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
            {"result": {"agent": {"pane_id": "pane"}}},
        ])
        with patch.object(hmux, "_find_or_create_workspace", return_value={
                "workspace_id": "workspace", "root_pane_id": "pane", "tab_id": "tab"}), \
             patch.object(hmux, "_resolve_herdr_target", return_value="worker"), \
             patch.object(backend, "pane_find_by_label", return_value="pane"), \
             patch.object(backend, "pane_run") as pane_run, \
             patch.object(backend, "agent_get", side_effect=lambda target: next(responses)), \
             patch.object(backend, "agent_rename") as rename:
            result = hmux.run_launch(make_cfg(pane), "team", "worker", False, shell="bash")

        self.assertEqual(result, 0)
        pane_run.assert_not_called()
        rename.assert_called_once_with("pane", "worker")

    def test_missing_roster_member_warns_when_register_missing_disabled(self):
        fake = SimpleNamespace(returncode=0, stdout='{"members":[]}', stderr="")
        stderr = io.StringIO()
        with patch.object(hmux.subprocess, "run", return_value=fake) as run, \
             contextlib.redirect_stderr(stderr):
            target = hmux._resolve_herdr_target("team", "worker", "config-ops", register_missing=False)

        self.assertEqual(target, "config-ops")
        self.assertIn("member 'worker' is not registered", stderr.getvalue())
        run.assert_called_once()  # roster read only; no add-member attempted

    def test_missing_roster_member_is_registered_with_backend_and_alias(self):
        members = SimpleNamespace(returncode=0, stdout='{"members":[]}', stderr="")
        add = SimpleNamespace(returncode=0, stdout="", stderr="")
        with patch.object(hmux.subprocess, "run", side_effect=[members, add]) as run:
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "config-ops")
        self.assertEqual(run.call_args_list[1].args[0], [
            "atm", "teams", "add-member", "team", "worker",
            "--backend", "herdr", "--alias", "config-ops",
        ])

    def test_missing_roster_member_without_alias_registers_and_returns_name(self):
        members = SimpleNamespace(returncode=0, stdout='{"members":[]}', stderr="")
        add = SimpleNamespace(returncode=0, stdout="", stderr="")
        with patch.object(hmux.subprocess, "run", side_effect=[members, add]) as run:
            target = hmux._resolve_herdr_target("team", "worker")

        self.assertEqual(target, "worker")
        self.assertEqual(run.call_args_list[1].args[0], [
            "atm", "teams", "add-member", "team", "worker", "--backend", "herdr",
        ])

    def test_missing_roster_member_add_failure_falls_back(self):
        members = SimpleNamespace(returncode=0, stdout='{"members":[]}', stderr="")
        add = SimpleNamespace(returncode=1, stdout="", stderr="add-member failed\n")
        stderr = io.StringIO()
        with patch.object(hmux.subprocess, "run", side_effect=[members, add]), \
             contextlib.redirect_stderr(stderr):
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "config-ops")
        self.assertIn("atm teams add-member failed for 'worker'", stderr.getvalue())

    def test_roster_alias_update_failure_prints_atm_error_and_uses_name(self):
        members = SimpleNamespace(
            returncode=0,
            stdout='{"members":[{"name":"worker","extra":{}}]}',
            stderr="",
        )
        update = SimpleNamespace(returncode=1, stdout="", stderr="alias writes disabled by policy\n")
        stderr = io.StringIO()
        with patch.object(hmux.subprocess, "run", side_effect=[members, update]), \
             contextlib.redirect_stderr(stderr):
            target = hmux._resolve_herdr_target("team", "worker", "config-ops")

        self.assertEqual(target, "worker")
        self.assertIn("alias writes disabled by policy", stderr.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)
