#!/usr/bin/env python3
"""S-7 — integration test: launch a Luna stand-in, nudge it, assert round-trip.

Runnable from a non-TTY shell (the MVP acceptance criterion). Exercises the
REAL `hmux` CLI + the REAL `atm-nudge-xml-1.py` herdr backend, then asserts a
unique nonce injected into the nudge envelope appears in the target pane.

The "Luna" stand-in is a raw command pane (label == ATM_IDENTITY == "luna"),
NOT a live codex agent — this proves the launch+nudge PLUMBING end-to-end
without agent cost. (A live-agent variant is a follow-up; see S-7 bead.)

Run:
    python3 test_hmux_integration.py

Skips (exit 0) when the `atm` binary or the nudge script is unavailable, so the
suite stays cross-platform (atm is not yet available on Windows).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPTS_DIR)
import hmux_backend as backend  # noqa: E402

TEAM = "hmux-integration"
NUDGE = os.path.expanduser("~/.local/bin/atm-nudge-xml-1.py")


def main() -> int:
    if shutil.which("atm") is None:
        print("SKIP: 'atm' binary not found on PATH (required for nudge round-trip)")
        return 0
    if not os.path.isfile(NUDGE):
        print(f"SKIP: nudge script not found at {NUDGE}")
        return 0

    workdir = tempfile.mkdtemp(prefix="hmux-s7-")
    config_path = os.path.join(workdir, ".atm.toml")
    with open(config_path, "w") as f:
        f.write(f"""[atm]
default_team = "{TEAM}"

[rmux]
session = "{TEAM}"

[[rmux.windows]]
name = "agents"
layout = "tiled"

[[rmux.windows.panes]]
name = "luna"
env = {{ ATM_IDENTITY = "luna", ATM_TEAM = "{TEAM}" }}
command = "echo __LUNA_READY__"
""")

    workspace_id = None
    try:
        # 1. Launch the stand-in via the REAL hmux CLI (non-TTY).
        proc = subprocess.run(
            [sys.executable, os.path.join(SCRIPTS_DIR, "hmux"),
             "--config", config_path],
            capture_output=True, text=True,
        )
        if proc.returncode != 0:
            print(f"FAIL: hmux launch failed: {proc.stderr}")
            return 1

        # 2. Resolve the workspace + pane by label.
        workspace_id = backend.workspace_find_by_label(TEAM)
        if workspace_id is None:
            print("FAIL: workspace not found after launch")
            return 1
        pane_id = backend.pane_find_by_label(workspace_id, "luna")
        if pane_id is None:
            print("FAIL: pane 'luna' not found")
            return 1

        time.sleep(1)

        # 3. Nudge via the REAL nudge script, injecting a unique nonce.
        nonce = f"rtt-{uuid.uuid4()}"
        env = dict(os.environ)
        env["ATM_POST_SEND"] = (
            f'{{"team":"{TEAM}","message_id":"{nonce}",'
            f'"description":"integration probe"}}'
        )
        nproc = subprocess.run(
            [NUDGE, "luna"], capture_output=True, text=True, env=env,
        )
        if nproc.returncode != 0:
            print(f"FAIL: nudge failed: {nproc.stderr}")
            return 1

        time.sleep(1)

        # 4. Assert the nonce appears in the target pane's output.
        text = backend.pane_read(pane_id)
        if nonce not in text:
            print(f"FAIL: nonce {nonce} not in pane {pane_id} output")
            print(text[-400:])
            return 1

        print(f"PASS: launch -> nudge -> round-trip confirmed")
        print(f"  workspace={workspace_id} pane={pane_id} nonce={nonce}")
        return 0
    finally:
        if workspace_id is not None:
            try:
                backend.workspace_close(workspace_id)
            except Exception:
                pass


if __name__ == "__main__":
    sys.exit(main())
