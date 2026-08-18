#!/usr/bin/env zsh

# Tests rmux runtime agent spawn aliases by inspecting the `--dry-run`
# command line. Verifies:
#   * codex aliases (luna/terra/sol) resolve to full gpt-5.6-* model names
#   * bare `codex` defaults to gpt-5.6-terra
#   * fable resolves to claude --model fable (alongside sonnet baseline)
#   * an explicit --model override beats the alias default

setopt ERR_EXIT PIPE_FAIL NO_UNSET

script_dir=${0:A:h}
rmux_script="${script_dir}/rmux"
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

config_file="${tmp_dir}/.atm.toml"
print -r -- '[rmux]
session = "rmux-test"

[core]
default_team = "rmux-test"' > "$config_file"

run_dry() {
  zsh "$rmux_script" "$1" test-agent --config "$config_file" --dry-run
}

assert_contains() {
  local label="$1"
  local output="$2"
  local needle="$3"
  if [[ "$output" != *"$needle"* ]]; then
    print -u2 -- "FAIL: ${label} — expected to find: ${needle}"
    print -u2 -- "$output"
    return 1
  fi
  print -- "ok: ${label}"
}

# Codex aliases -> full gpt-5.6-* model names.
out=$(run_dry codex); assert_contains "codex -> gpt-5.6-terra" "$out" "codex -c features.codex_hooks=true --yolo --model gpt-5.6-terra"
out=$(run_dry luna);  assert_contains "luna  -> gpt-5.6-luna"  "$out" "codex -c features.codex_hooks=true --yolo --model gpt-5.6-luna"
out=$(run_dry terra); assert_contains "terra -> gpt-5.6-terra" "$out" "codex -c features.codex_hooks=true --yolo --model gpt-5.6-terra"
out=$(run_dry sol);   assert_contains "sol   -> gpt-5.6-sol"   "$out" "codex -c features.codex_hooks=true --yolo --model gpt-5.6-sol"

# Explicit --model override wins over the alias default.
out=$(zsh "$rmux_script" luna test-agent --config "$config_file" --dry-run --model gpt-5.6-sol)
assert_contains "luna --model gpt-5.6-sol override" "$out" "codex -c features.codex_hooks=true --yolo --model gpt-5.6-sol"

# Claude aliases: fable is the new one; sonnet is the pre-existing baseline.
out=$(run_dry fable);  assert_contains "fable  -> claude --model fable"  "$out" "claude --model fable"
out=$(run_dry sonnet); assert_contains "sonnet -> claude --model sonnet" "$out" "claude --model sonnet"

# Regression guard: the short codex model name must never leak into the
# emitted command (codex rejects bare `luna`/`terra`/`sol`).
out=$(run_dry luna)
if [[ "$out" == *" --model luna"* ]]; then
  print -u2 -- "FAIL: short model name 'luna' leaked into command"
  print -u2 -- "$out"
  exit 1
fi

print -- 'rmux agent-alias tests passed'
