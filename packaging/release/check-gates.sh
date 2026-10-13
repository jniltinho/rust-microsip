#!/bin/sh
# Release gate check for a final version: docs/release/vX.Y.Z.md must check
# every task id of the release changes ("- [x] <change> <id> ...", one id per
# item), every AG-0N gate and the owner's decision, and contain no open
# checkbox in any task-list syntax (also inside blockquotes).
# Usage: sh packaging/release/check-gates.sh X.Y.Z
# CHECK_GATES_ROOT points at another tree (used by check-gates-selftest.sh).
set -eu
cd "${CHECK_GATES_ROOT:-$(dirname "$0")/../..}"
version=${1:?usage: check-gates.sh X.Y.Z}
notes="docs/release/v$version.md"
[ -f "$notes" ] || { echo "missing $notes" >&2; exit 1; }
item='^[>[:space:]]*([-*+]|[0-9]+[.)])[[:space:]]+'
if grep -nE "$item\\[[[:space:]]*\\]" "$notes"; then
    echo "open release gates in $notes" >&2
    exit 1
fi
# The item must start with exactly this label (dots escaped).
checked() {
    label=$(printf '%s' "$1" | sed 's/\./\\./g')
    grep -qE "$item\\[[xX]\\][[:space:]]+$label([[:space:]:]|$)" "$notes"
}
missing=0
for change in create-native-rust-linux-release add-agent-cli-mcp; do
    tasks="openspec/changes/$change/tasks.md"
    [ -f "$tasks" ] || { echo "missing $tasks" >&2; exit 1; }
    ids=$(sed -nE 's/^[[:space:]]*[-*+][[:space:]]+\[[ xX]\][[:space:]]+([0-9]+(\.[0-9]+)+)([[:space:]].*)?$/\1/p' "$tasks")
    [ -n "$ids" ] || { echo "no task ids found in $tasks" >&2; exit 1; }
    # Every checkbox of the change must carry an id, or it would go unchecked.
    boxes=$(grep -cE "$item\\[[[:space:]xX]*\\]" "$tasks" || true)
    [ "$boxes" = "$(printf '%s\n' "$ids" | wc -l | tr -d ' ')" ] \
        || { echo "$tasks has checkboxes without a task id" >&2; exit 1; }
    for id in $ids; do
        checked "$change $id" || { echo "gate not checked: $change $id" >&2; missing=1; }
    done
done
for gate in AG-01 AG-02 AG-03 AG-04 AG-05 AG-06 "Owner decision"; do
    checked "$gate" || { echo "gate not checked: $gate" >&2; missing=1; }
done
[ "$missing" = 0 ] || exit 1
echo "all release gates checked in $notes"
