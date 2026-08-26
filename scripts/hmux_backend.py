#!/usr/bin/env python3
"""hmux_backend — thin wrapper over the `herdr` CLI (S-3).

Isolates every herdr call behind typed functions + JSON parsing, so the
session builder / spawn / nudge layers never shell out directly.

Empirically verified against herdr 0.8.2 (2026-08). Key observed facts:
  * workspace create -> result.workspace.workspace_id, result.root_pane.pane_id,
    result.tab.tab_id
  * tab create      -> result.tab.tab_id, result.root_pane.pane_id (new root pane)
  * pane split      -> result.pane.pane_id
  * pane rename     -> positional label; result.pane.label
  * pane list       -> result.panes[]; `label` key is ABSENT for unrenamed panes
                       (callers MUST use .get("label", ""))
  * pane run        -> empty stdout (executes text + Enter)
  * pane read       -> raw text (NOT JSON)
  * workspace close -> result.type == "ok"
"""

from __future__ import annotations

import json
import subprocess
from typing import Any, Dict, List, Optional


class HerdrError(Exception):
    """A herdr CLI call failed."""


def _json(args: List[str]) -> Dict[str, Any]:
    """Run `herdr <args>` and parse JSON output. Raises HerdrError on failure."""
    proc = subprocess.run(["herdr", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise HerdrError(f"herdr {' '.join(args)} failed: {proc.stderr.strip()}")
    out = proc.stdout.strip()
    if not out:
        return {}
    try:
        return json.loads(out)
    except json.JSONDecodeError as e:
        raise HerdrError(f"herdr {' '.join(args)} returned non-JSON: {out[:200]!r}") from e


def _raw(args: List[str]) -> str:
    """Run `herdr <args>` and return raw stdout (for `pane read`)."""
    proc = subprocess.run(["herdr", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise HerdrError(f"herdr {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout


# ── workspace ──────────────────────────────────────────────────────────

def workspace_create(label: str) -> Dict[str, str]:
    """Create a workspace. Returns {workspace_id, root_pane_id, tab_id}."""
    data = _json(["workspace", "create", "--label", label])
    ws = data["result"]["workspace"]
    root = data["result"]["root_pane"]
    tab = data["result"]["tab"]
    return {
        "workspace_id": ws["workspace_id"],
        "root_pane_id": root["pane_id"],
        "tab_id": tab["tab_id"],
    }


def workspace_list() -> List[Dict[str, str]]:
    """List workspaces. Returns [{workspace_id, label}, ...]."""
    data = _json(["workspace", "list"])
    out: List[Dict[str, str]] = []
    for ws in data["result"].get("workspaces", []):
        out.append({"workspace_id": ws["workspace_id"], "label": ws.get("label", "")})
    return out


def workspace_find_by_label(label: str) -> Optional[str]:
    """Return workspace_id whose label == label, or None."""
    for ws in workspace_list():
        if ws["label"] == label:
            return ws["workspace_id"]
    return None


def workspace_find_or_create(label: str) -> Dict[str, str]:
    """Find a workspace by label, creating it if absent."""
    existing = workspace_find_by_label(label)
    if existing:
        # Need root pane + tab for the existing workspace: list panes.
        panes = pane_list(existing)
        if panes:
            return {"workspace_id": existing,
                    "root_pane_id": panes[0]["pane_id"],
                    "tab_id": panes[0]["tab_id"]}
        return {"workspace_id": existing, "root_pane_id": "", "tab_id": ""}
    return workspace_create(label)


def workspace_close(workspace_id: str) -> None:
    _json(["workspace", "close", workspace_id])


# ── tab ────────────────────────────────────────────────────────────────

def tab_create(workspace_id: str, label: str) -> Dict[str, str]:
    """Create a tab. Returns {tab_id, root_pane_id}."""
    data = _json(["tab", "create", "--workspace", workspace_id, "--label", label])
    tab = data["result"]["tab"]
    root = data["result"]["root_pane"]
    return {"tab_id": tab["tab_id"], "root_pane_id": root["pane_id"]}


def tab_list(workspace_id: str) -> List[Dict[str, Any]]:
    """List tabs in a workspace. Returns [{tab_id, label, number, pane_count}, ...]."""
    data = _json(["tab", "list", "--workspace", workspace_id])
    out: List[Dict[str, Any]] = []
    for t in data["result"].get("tabs", []):
        out.append({
            "tab_id": t.get("tab_id", ""),
            "label": t.get("label", ""),
            "number": t.get("number", 0),
            "pane_count": t.get("pane_count", 0),
        })
    return out


# ── pane ───────────────────────────────────────────────────────────────

def pane_split(pane_id: str, direction: str) -> str:
    """Split a pane. direction in {left,right,up,down}. Returns new pane_id."""
    data = _json(["pane", "split", "--pane", pane_id, "--direction", direction])
    return data["result"]["pane"]["pane_id"]


def pane_rename(pane_id: str, label: str) -> None:
    _json(["pane", "rename", pane_id, label])


def pane_list(workspace_id: str) -> List[Dict[str, str]]:
    """List panes in a workspace. Returns [{pane_id, tab_id, label}, ...].

    NOTE: `label` is ABSENT for unrenamed panes; normalized to "" here.
    """
    data = _json(["pane", "list", "--workspace", workspace_id])
    out: List[Dict[str, str]] = []
    for p in data["result"].get("panes", []):
        out.append({
            "pane_id": p["pane_id"],
            "tab_id": p.get("tab_id", ""),
            "label": p.get("label", ""),
        })
    return out


def pane_find_by_label(workspace_id: str, label: str) -> Optional[str]:
    """Return pane_id whose label == label, or None."""
    for p in pane_list(workspace_id):
        if p["label"] == label:
            return p["pane_id"]
    return None


def pane_run(pane_id: str, command: str) -> None:
    """Send command + Enter to a pane (executes it)."""
    _json(["pane", "run", pane_id, command])


def pane_send_text(pane_id: str, text: str) -> None:
    """Send literal text to a pane, NO Enter."""
    _json(["pane", "send-text", pane_id, text])


def pane_read(pane_id: str, source: str = "recent-unwrapped") -> str:
    """Read pane output. Returns raw text."""
    return _raw(["pane", "read", pane_id, "--source", source])
