#!/usr/bin/env zsh

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

assert_model() {
  local agent_type="$1"
  local expected_model="$2"
  local output

  output=$(zsh "$rmux_script" "$agent_type" test-agent \
    --config "$config_file" --dry-run)
  if [[ "$output" != *"cmd:     codex -c features.codex_hooks=true --yolo --model ${expected_model}"* ]]; then
    print -u2 -- "expected ${agent_type} to select ${expected_model}"
    print -u2 -- "$output"
    return 1
  fi
}

assert_model luna luna
assert_model terra terra
assert_model sol sol
assert_model codex terra

print -- 'rmux Codex model alias tests passed'
