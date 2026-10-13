#!/bin/sh
# Lab suite: the release app and the production engine against a fresh
# disposable PBX per scenario, with one JSON report per scenario plus a
# summary bound to the commit and the binaries' SHA-256. Fictional fixtures
# only (sh docker/asterisk/lab.sh build && sh docker/asterisk/lab.sh init).
#
# Usage: sh docker/asterisk/suite.sh [SCENARIO...]
#   Scenarios: engine-lab engine-network gui-link gui-frozen-exit gui-theme
#   gui-a11y gui-a11y-call (default: all). SKIP_BUILD=1 reuses the image,
#   which must carry the current commit in its label.
# Exit status 0 only when every requested scenario wrote a passing report
# and no lab container, network or volume is left.
set -u
cd "$(dirname "$0")/../.."
[ "$(id -u)" -ne 0 ] || { echo "ERROR: run the suite as a non-root user." >&2; exit 1; }
all="engine-lab engine-network gui-link gui-frozen-exit gui-theme gui-a11y gui-a11y-call"
scenarios=${*:-$all}
for scenario in $scenarios; do
    case " $all " in *" $scenario "*) ;; *) echo "unknown scenario: $scenario" >&2; exit 2 ;; esac
done
clients=artifacts/asterisk-lab/clients
[ -f "$clients/credentials.json" ] || { echo "ERROR: run 'sh docker/asterisk/lab.sh init' first." >&2; exit 1; }
image=rust-microsip-suite:local
net=rust-microsip-lab_isolated
project=com.docker.compose.project=rust-microsip-lab
export LAB_UID="$(id -u)" LAB_GID="$(id -g)"
B="docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.calls-g0.yaml"
commit=$(git rev-parse HEAD)
# Changes (tracked or untracked) in the Docker build context and in the lab
# (PBX configuration and suite scripts). A run with changes fails unless ALLOW_DIRTY=1 (development only).
context_changes=$(git status --porcelain --untracked-files=all -- Dockerfile .dockerignore \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo apps crates packaging \
    docker/rust-components.sha256 docker/asterisk | wc -l)

if [ "${SKIP_BUILD:-}" = 1 ]; then
    built=$(docker image inspect -f '{{index .Config.Labels "rust-microsip.commit"}}' "$image" 2>/dev/null)
    [ "$built" = "$commit" ] || { echo "ERROR: $image was built from '${built:-?}', not HEAD; rebuild." >&2; exit 1; }
else
    docker buildx build --progress plain --platform linux/amd64 --target suite-runner \
        --label "rust-microsip.commit=$commit" --label "rust-microsip.context-changes=$context_changes" \
        --load -t "$image" . > artifacts/suite-build.log 2>&1 \
        || { tail -30 artifacts/suite-build.log; echo "ERROR: suite image build failed" >&2; exit 1; }
fi
image_changes=$(docker image inspect -f '{{index .Config.Labels "rust-microsip.context-changes"}}' "$image")

run=artifacts/suite/$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)
mkdir -p "$run/cfg" && chmod 700 "$run" "$run/cfg"
pbx_down() { $B down --volumes --remove-orphans > /dev/null 2>&1; }
cleanup() {
    docker rm -f rust-microsip-suite-gui rust-microsip-suite-network rust-microsip-suite-engine > /dev/null 2>&1
    pbx_down
    rm -rf "$run/cfg" "$run/password.env"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# Fictional lab account 1001 (UDP, plain RTP); no [ui] section so the theme
# can be changed from the window.
python3 -I - "$clients/credentials.json" "$run" <<'EOF'
import json, os, sys
password = json.load(open(sys.argv[1]))["1001"]
run = sys.argv[2]
with open(os.open(f"{run}/cfg/config.toml", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as f:
    f.write('schema = 1\n[account]\ndisplay_name = "Lab 1001"\nusername = "1001"\n'
            'domain = "pbx"\nserver = "pbx"\nport = 5060\ntransport = "udp"\n'
            'media = "rtp"\nexpires = 120\n')
with open(os.open(f"{run}/password.env", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as f:
    f.write(f"RUST_MICROSIP_SIP_PASSWORD={password}\n")
EOF
binaries=$(docker run --rm --network none --entrypoint sha256sum "$image" \
    /usr/local/bin/rust-microsip /usr/local/bin/engine-lab /usr/local/bin/engine-network)
scripts=$(sha256sum docker/asterisk/suite.sh docker/asterisk/suite/gui.sh docker/asterisk/suite/atspi.py)
image_id=$(docker image inspect -f '{{.Id}}' "$image")

pbx_up() { $B up --no-build -d --wait pbx > /dev/null 2>&1; }
cli() { docker exec "$($B ps -q pbx)" asterisk -C /run/asterisk/config/asterisk.conf -rx "$1"; }
channels() { cli 'core show channels count' | sed -n 's/^\([0-9]*\) active channel.*/\1/p'; }
contacts() { cli 'pjsip show contacts' | grep -c 'Contact:.*1001'; }
container() { # name, extra docker arguments...
    name=$1; shift
    docker run --name "$name" --label "$project" \
        --network "$net" --user "$LAB_UID:$LAB_GID" --read-only --cap-drop ALL \
        --security-opt no-new-privileges --memory 2g --pids-limit 512 \
        --tmpfs /tmp:rw,mode=1777 "$@"
}
timeouts=0
wait_phase() { # dir, phase, seconds; a timeout fails the scenario
    timeout "$3" sh -c "until [ -e '$1/phase-$2' ]; do sleep 0.3; done" || timeouts=$((timeouts + 1))
}
ack() { touch "$out/ack-$1"; }
j() { echo "${1:-null}"; } # JSON value or null
value() { sed -n "s/^\(.* \)\{0,1\}$1=\([^ ]*\).*/\2/p" "$out/results.txt" 2>/dev/null | tail -1; }
report() { # scenario, result, observations (JSON object)
    if [ "$timeouts" != 0 ] || [ -s "$out/failure" ]; then result=fail; fi
    python3 -I - "$1" "$result" "$3" "$commit" "$context_changes" "$image_changes" "$image_id" \
        "${SKIP_BUILD:-0}" "$timeouts" "$binaries" "$scripts" "$out" <<'EOF'
import hashlib, json, os, sys
(scenario, result, observations, commit, changes, image_changes, image, skip, timeouts,
 binaries, scripts, out) = sys.argv[1:]
try:
    observations = json.loads(observations)
except ValueError:
    observations, result = {"unparsable_observations": observations}, "fail"
files = {}
for name in sorted(os.listdir(out)):
    if name.endswith((".log", ".txt", ".png")):
        with open(os.path.join(out, name), "rb") as f:
            files[name] = hashlib.sha256(f.read()).hexdigest()
pairs = lambda text: {line.split()[1]: line.split()[0] for line in text.splitlines()}
row = {
    "scenario": scenario, "result": result, "observations": observations,
    "commit": commit, "build_context_changes_now": int(changes),
    "build_context_changes_at_image_build": int(image_changes) if image_changes.isdigit() else None,
    "image": image, "image_reused": skip == "1", "phase_timeouts": int(timeouts),
    "binaries": pairs(binaries), "scripts": pairs(scripts), "files_sha256": files,
}
with open(os.path.join(out, "report.json"), "w") as f:
    json.dump(row, f, indent=2)
print(f"{scenario}: {result}")
EOF
    timeouts=0
}

engine_lab() {
    out=$run/engine-lab; mkdir -p "$out"; pbx_up
    # engine_lab prints a scenario's row only after that scenario passed and
    # exits non-zero on the first failure.
    timeout -k 30 900 docker run --rm --init --name rust-microsip-suite-engine --label "$project" \
        --network "$net" --user "$LAB_UID:$LAB_GID" --read-only --cap-drop ALL \
        --security-opt no-new-privileges --memory 2g --pids-limit 512 --tmpfs /tmp:rw,mode=1777 \
        --mount "type=bind,source=$PWD/$clients,target=/lab,readonly" \
        --entrypoint /usr/local/bin/engine-lab "$image" > "$out/engine-lab.log" 2>&1
    code=$?
    pbx_down
    rows=$(python3 -I - "$out/engine-lab.log" <<'EOF'
import json, sys
rows = []
for line in open(sys.argv[1]):
    try:
        rows.append(json.loads(line)) if line.startswith("{") else None
    except ValueError:
        pass
print(json.dumps(rows))
EOF
)
    seen=$(echo "$rows" | python3 -I -c 'import json,sys; print(" ".join(sorted(r.get("scenario", "?") for r in json.load(sys.stdin))))')
    result=fail
    [ "$code" = 0 ] && [ "$seen" = "downgrade-rejection tls-negatives tls-srtp udp-rtp wrong-password" ] && result=pass
    report engine-lab "$result" "{\"exit\": $(j "$code"), \"rows\": ${rows:-[]}}"
}

engine_network() {
    out=$run/engine-network; mkdir -p "$out"; pbx_up
    name=rust-microsip-suite-network
    container $name -d \
        --mount "type=bind,source=$PWD/$clients,target=/lab,readonly" \
        --entrypoint /usr/local/bin/engine-network "$image" > /dev/null
    ip=$(docker inspect -f "{{(index .NetworkSettings.Networks \"$net\").IPAddress}}" $name)
    timeout 120 sh -c "until docker logs $name 2>&1 | grep -q renewal_window; do sleep 1; done" || timeouts=$((timeouts + 1))
    renewed=$(contacts)
    timeout 60 sh -c "until docker logs $name 2>&1 | grep -q in_call; do sleep 0.5; done" || timeouts=$((timeouts + 1))
    in_call=$(channels)
    docker network disconnect $net $name || timeouts=$((timeouts + 1))
    sleep 40
    docker network connect --ip "$ip" $net $name || timeouts=$((timeouts + 1))
    timeout 400 sh -c "until docker logs $name 2>&1 | grep -q recovered; do sleep 1; done" || timeouts=$((timeouts + 1))
    sleep 3
    recovered_contacts=$(contacts); recovered_channels=$(channels)
    code=$(timeout 120 docker wait $name) || timeouts=$((timeouts + 1))
    docker logs $name > "$out/engine-network.log" 2>&1; docker rm -f $name > /dev/null
    end_contacts=$(contacts)
    pbx_down
    result=fail
    [ "$code" = 0 ] && [ "$renewed" = 1 ] && [ "$in_call" = 1 ] && [ "$recovered_contacts" = 1 ] \
        && [ "$recovered_channels" = 0 ] && [ "$end_contacts" = 0 ] && result=pass
    report engine-network "$result" "{\"exit\": $(j "$code"), \"contact_after_renewal_window\": $(j "$renewed"), \"channels_in_call\": $(j "$in_call"), \"contact_after_recovery\": $(j "$recovered_contacts"), \"channels_after_recovery\": $(j "$recovered_channels"), \"contact_after_exit\": $(j "$end_contacts")}"
}

gui() { # scenario name
    out=$run/gui-$1; mkdir -p "$out"
    container rust-microsip-suite-gui -d --env-file "$run/password.env" \
        -e HOME=/tmp/home -e XDG_RUNTIME_DIR=/tmp/xdg \
        -v "$PWD/$run/cfg:/cfg:ro" -v "$PWD/docker/asterisk/suite:/suite:ro" -v "$PWD/$out:/capture" \
        --entrypoint dbus-run-session "$image" -- sh /suite/gui.sh "$1" > /dev/null
}
gui_finish() {
    wait_phase "$out" exited 240
    docker rm -f rust-microsip-suite-gui > /dev/null 2>&1
}
graceful() { grep -q 'rust-microsip: closed after signal' "$out/$1.log"; }

gui_link() {
    pbx_up; gui link
    wait_phase "$out" registered 120; registered=$(contacts); ack registered
    wait_phase "$out" link-filled 60; before_enter=$(channels); ack link-filled
    wait_phase "$out" in-call 60; in_call=$(channels)
    to_602=$(cli 'core show channels concise' | grep -c '!602!'); ack in-call
    wait_phase "$out" ended 60; ended=$(channels); ack ended
    gui_finish; after_exit=$(contacts); pbx_down
    forward=$(value forward_exit); app_exit=$(value app_exit)
    result=fail
    [ "$registered" = 1 ] && [ "$forward" = 0 ] && [ "$before_enter" = 0 ] && [ "$in_call" = 1 ] \
        && [ "$to_602" -ge 1 ] && [ "$ended" = 0 ] && [ "$app_exit" = 143 ] && graceful app \
        && [ "$after_exit" = 0 ] && result=pass
    report gui-link "$result" "{\"registered\": $(j "$registered"), \"second_launch_exit\": $(j "$forward"), \"channels_before_enter\": $(j "$before_enter"), \"channels_in_call\": $(j "$in_call"), \"channels_to_602\": $(j "$to_602"), \"channels_after_escape\": $(j "$ended"), \"app_exit\": $(j "$app_exit"), \"contact_after_exit\": $(j "$after_exit")}"
}

gui_frozen_exit() {
    pbx_up; gui frozen-exit
    wait_phase "$out" in-call 120; in_call=$(channels)
    docker pause "$($B ps -q pbx)" > /dev/null; touch "$out/pbx-frozen"
    gui_finish
    docker unpause "$($B ps -q pbx)" > /dev/null; pbx_down
    code=$(value frozen_exit); elapsed=$(value elapsed_ms)
    marker=false; graceful app && marker=true
    result=fail
    [ "$in_call" = 1 ] && [ "$code" = 143 ] && [ $marker = true ] && [ "${elapsed:-99999}" -lt 6000 ] \
        && ! grep -q left_running "$out/results.txt" && result=pass
    report gui-frozen-exit "$result" "{\"channels_in_call\": $(j "$in_call"), \"app_exit\": $(j "$code"), \"graceful_marker\": $marker, \"exit_ms_with_pbx_paused\": $(j "$elapsed")}"
}

gui_theme() {
    pbx_up; gui theme
    wait_phase "$out" dark-in-call 180; during=$(channels); ack dark-in-call
    gui_finish; pbx_down
    system=$(value system_theme_brightness); dark=$(value dark_in_call_brightness); again=$(value relaunched_brightness)
    result=fail
    # Dark but rendered: a black, unrendered capture would be near 0%.
    [ "$during" = 1 ] && [ "${system:-0}" -gt 60 ] && [ "${dark:-0}" -gt 5 ] && [ "${dark:-100}" -lt 40 ] \
        && [ "${again:-0}" -gt 5 ] && [ "${again:-100}" -lt 40 ] && result=pass
    report gui-theme "$result" "{\"system_theme_without_portal_brightness_pct\": $(j "$system"), \"channels_after_switch\": $(j "$during"), \"dark_in_call_brightness_pct\": $(j "$dark"), \"relaunched_brightness_pct\": $(j "$again")}"
}

# Tab cycle: the first control to repeat is the first one focused (a full
# cycle, not a trap or a sub-loop), after at least MIN distinct controls that
# include REQUIRED; with "back", Shift+Tab must return to the control before
# the last one.
tab_cycle() { # prefix, minimum distinct, back|noback, required control names...
    python3 -I - "$out/focus.txt" "$@" <<'EOF'
import sys
path, prefix, minimum, back_mode, *required = sys.argv[1:]
try:
    lines = [l.rstrip("\n").split(" ", 1) for l in open(path) if " " in l]
except OSError:
    lines = []
seq = [v for k, v in lines if k.startswith(prefix) and k[len(prefix):].isdigit()]
first_repeat = next((i for i, v in enumerate(seq) if v in seq[:i]), None)
cycle = seq[:first_repeat] if first_repeat is not None else []
full = first_repeat is not None and seq[first_repeat] == seq[0] and len(cycle) >= int(minimum)
names = " ".join(cycle)
has_required = all(f"'{name}'" in names for name in required)
back = [v for k, v in lines if k == "shift-tab"]
reverse = back_mode != "back" or (len(back) == 1 and len(seq) > 1 and back[0] == seq[-2])
print("true" if full and has_required and reverse and "none" not in seq else "false")
EOF
}

gui_a11y() {
    pbx_up; gui a11y; gui_finish; pbx_down
    problems=$(sed -n 's/^SCREENS 7 PROBLEMS \([0-9]*\)$/\1/p' "$out/screens.txt" 2>/dev/null)
    cycle=$(tab_cycle tab 15 back Phone Calls Contacts Menu "Number to call" DND)
    result=fail
    [ "$(value screens_exit)" = 0 ] && [ "$problems" = 0 ] && [ "$cycle" = true ] && result=pass
    report gui-a11y "$result" "{\"screens_checked\": $(grep -c '^== ' "$out/screens.txt" 2>/dev/null), \"problems\": $(j "$problems"), \"tab_cycle_wraps_and_reverses\": $(j "$cycle")}"
}

gui_a11y_call() {
    pbx_up; gui a11y-call
    wait_phase "$out" in-call 120; in_call=$(channels); ack in-call
    wait_phase "$out" ended 120; ended=$(channels); ack ended
    gui_finish; pbx_down
    reached=0
    for control in Call End XFER HOLD MUTE; do
        grep -q "'$control'" "$out/focus.txt" 2>/dev/null && reached=$((reached + 1))
    done
    cycle=$(tab_cycle call 15 noback Phone Calls Contacts Menu End XFER HOLD MUTE DND)
    result=fail
    [ "$in_call" = 1 ] && [ "$ended" = 0 ] && [ "$(value call_sweep_exit)" = 0 ] \
        && [ "$(value transfer_sweep_exit)" = 0 ] && [ $reached = 5 ] && [ "$cycle" = true ] && result=pass
    report gui-a11y-call "$result" "{\"channels_in_call\": $(j "$in_call"), \"channels_after_end\": $(j "$ended"), \"call_controls_reached_by_tab\": $reached, \"tab_cycle_wraps\": $(j "$cycle")}"
}

for scenario in $scenarios; do
    case $scenario in
        engine-lab) engine_lab ;;
        engine-network) engine_network ;;
        gui-link) gui_link ;;
        gui-frozen-exit) gui_frozen_exit ;;
        gui-theme) gui_theme ;;
        gui-a11y) gui_a11y ;;
        gui-a11y-call) gui_a11y_call ;;
    esac
done
cleanup
leftover=$(( $(docker ps -aq --filter "label=$project" | wc -l) \
    + $(docker network ls -q --filter "label=$project" | wc -l) \
    + $(docker volume ls -q --filter "label=$project" | wc -l) ))
python3 -I - "$run" "$leftover" "$commit" $scenarios <<'EOF'
import json, os, sys
run, leftover, commit, requested = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4:]
results = {}
for name in requested:
    path = os.path.join(run, name, "report.json")
    results[name] = json.load(open(path))["result"] if os.path.isfile(path) else "missing"
reports = [json.load(open(os.path.join(run, n, "report.json")))
           for n in requested if results[n] != "missing"]
dirty = any(r["build_context_changes_now"] or r["build_context_changes_at_image_build"] != 0
            for r in reports)
allow_dirty = os.environ.get("ALLOW_DIRTY") == "1"
ok = all(r == "pass" for r in results.values()) and leftover == 0 and (allow_dirty or not dirty)
summary = {"run": os.path.basename(run), "commit": commit, "uncommitted_changes": dirty,
           "passed": sum(r == "pass" for r in results.values()), "requested": len(requested),
           "leftover_lab_objects": leftover, "result": "pass" if ok else "fail", "scenarios": results}
json.dump(summary, open(os.path.join(run, "summary.json"), "w"), indent=2)
print(json.dumps(summary))
sys.exit(0 if ok else 1)
EOF
