#!/usr/bin/env bash
# new-session.sh — Create a fresh 'scmux' tmux session with 3 panes:
#   pane 1: team-lead  |  pane 2: arch-scmux  |  pane 3: qa
#
# Usage:
#   ./scripts/new-session.sh          # create and attach immediately
#   ./scripts/new-session.sh --detach # create detached only

set -euo pipefail

SESSION="scmux"
DETACH=false
[[ "${1:-}" == "--detach" ]] && DETACH=true

# ── Guard: refuse to clobber an existing session ──────────────────────────────
if tmux has-session -t "$SESSION" 2>/dev/null; then
  echo "Error: tmux session '$SESSION' already exists."
  echo ""
  echo "To attach to it:   tmux attach -t $SESSION"
  echo "To kill it first:  tmux kill-session -t $SESSION && $0"
  exit 1
fi

# ── Create session — pane 1 (team-lead) ──────────────────────────────────────
tmux new-session -d -s "$SESSION" -x 220 -y 50
tmux rename-window -t "$SESSION" "$SESSION"
tmux send-keys -t "$SESSION" "printf '\\033]2;team-lead\\033\\\\'" Enter

# ── Pane 2: arch-scmux ───────────────────────────────────────────────────────
tmux split-window -h -t "$SESSION"
tmux send-keys -t "$SESSION" "printf '\\033]2;arch-scmux\\033\\\\'" Enter

# ── Pane 3: qa ───────────────────────────────────────────────────────────────
tmux split-window -h -t "$SESSION"
tmux send-keys -t "$SESSION" "printf '\\033]2;qa\\033\\\\'" Enter

# ── Even layout ───────────────────────────────────────────────────────────────
tmux select-layout -t "$SESSION" even-horizontal

# ── Focus pane 1 (team-lead, leftmost) ───────────────────────────────────────
tmux select-pane -t "$SESSION:.1"

# ── Report ────────────────────────────────────────────────────────────────────
PANE_COUNT=$(tmux list-panes -t "$SESSION" | wc -l | tr -d ' ')
echo "Session '$SESSION' created with $PANE_COUNT panes: team-lead | arch-scmux | qa"

if $DETACH; then
  echo "Attach with:  tmux attach -t $SESSION"
else
  exec tmux attach -t "$SESSION"
fi
