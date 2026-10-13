#!/bin/sh
# Negative and positive controls for check-gates.sh on a synthetic tree.
set -eu
script=$(cd "$(dirname "$0")" && pwd)/check-gates.sh
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/docs/release" "$root/openspec/changes/create-native-rust-linux-release" \
    "$root/openspec/changes/add-agent-cli-mcp"
printf -- '- [x] 1.1 First.\n  - [ ] 1.2 Second.\n' > "$root/openspec/changes/create-native-rust-linux-release/tasks.md"
printf -- '- [ ] 1.1 Agent.\n' > "$root/openspec/changes/add-agent-cli-mcp/tasks.md"
notes="$root/docs/release/v9.9.9.md"
good='- [x] create-native-rust-linux-release 1.1 done
- [x] create-native-rust-linux-release 1.2 done
- [x] add-agent-cli-mcp 1.1 done
- [x] AG-01
- [x] AG-02
- [x] AG-03
- [x] AG-04
- [x] AG-05
- [x] AG-06
- [x] Owner decision: publish'
run() { CHECK_GATES_ROOT="$root" sh "$script" 9.9.9 >/dev/null 2>&1; }
expect_fail() {
    printf '%s\n' "$2" > "$notes"
    if run; then echo "FAIL (accepted): $1" >&2; exit 1; fi
}
printf '%s\n' "$good" > "$notes"
run || { echo "FAIL (rejected the complete list)" >&2; exit 1; }
expect_fail "open checkbox in a blockquote" "$good
> - [ ] later"
expect_fail "open numbered checkbox" "$good
1. [ ] later"
expect_fail "one id covering both changes" "$(printf '%s\n' "$good" | grep -v 'add-agent-cli-mcp 1.1')
- [x] 1.1 both"
expect_fail "unescaped dot" "$(printf '%s\n' "$good" | sed 's/linux-release 1\.2/linux-release 102/')"
expect_fail "id mentioned inside another item" "$(printf '%s\n' "$good" | grep -v 'linux-release 1.2')
- [x] create-native-rust-linux-release 1.1 (see create-native-rust-linux-release 1.2)"
expect_fail "open checkbox with 1) marker" "$good
1) [ ] later"
expect_fail "owner decision missing" "$(printf '%s\n' "$good" | grep -v Owner)"
printf -- '- [ ] 1.1 Agent.\n- [ ] 1.2\n' > "$root/openspec/changes/add-agent-cli-mcp/tasks.md"
expect_fail "id at the end of the line is required too" "$good"
printf -- '- [ ] 1.1 Agent.\n1. [ ] 2.1 Numbered.\n' > "$root/openspec/changes/add-agent-cli-mcp/tasks.md"
expect_fail "numbered task without a parsed id" "$good"
printf -- '- [ ] 1.1 Agent.\n' > "$root/openspec/changes/add-agent-cli-mcp/tasks.md"
mv "$root/openspec/changes/add-agent-cli-mcp/tasks.md" "$root/tasks.bak"
expect_fail "tasks.md missing" "$good"
printf 'no tasks\n' > "$root/openspec/changes/add-agent-cli-mcp/tasks.md"
expect_fail "tasks.md without ids" "$good"
echo "check-gates selftest passed"
