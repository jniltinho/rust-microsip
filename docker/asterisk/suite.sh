#!/bin/sh
# Lab suite: the release app and the production engine against a fresh
# disposable PBX per scenario, with one JSON report per scenario plus a
# summary bound to the commit and the binaries' SHA-256. Fictional fixtures
# only (sh docker/asterisk/lab.sh build && sh docker/asterisk/lab.sh init).
#
# Usage: sh docker/asterisk/suite.sh [SCENARIO...]
#   Scenarios: engine-lab engine-network gui-link gui-frozen-exit gui-theme
#   gui-a11y gui-a11y-call (default: all). SKIP_BUILD=1 reuses the image.
set -u
cd "$(dirname "$0")/../.."
[ "$(id -u)" -ne 0 ] || { echo "ERROR: run the suite as a non-root user." >&2; exit 1; }
clients=artifacts/asterisk-lab/clients
[ -f "$clients/credentials.json" ] || { echo "ERROR: run 'sh docker/asterisk/lab.sh init' first." >&2; exit 1; }
image=rust-microsip-suite:local
net=rust-microsip-lab_isolated
export LAB_UID="$(id -u)" LAB_GID="$(id -g)"
B="docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.calls-g0.yaml"
scenarios=${*:-engine-lab engine-network gui-link gui-frozen-exit gui-theme gui-a11y gui-a11y-call}

if [ "${SKIP_BUILD:-}" != 1 ]; then
    docker buildx build --progress plain --platform linux/amd64 --target suite-runner \
        --load -t "$image" . > artifacts/suite-build.log 2>&1 \
        || { tail -30 artifacts/suite-build.log; echo "ERROR: suite image build failed" >&2; exit 1; }
fi
commit=$(git rev-parse HEAD)
dirty=$(git status --porcelain --untracked-files=no | wc -l)
run=artifacts/suite/$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)
mkdir -p -m 700 "$run/cfg"
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
image_id=$(docker image inspect -f '{{.Id}}' "$image")

pbx_up() { $B up --no-build -d --wait pbx > /dev/null 2>&1; }
pbx_down() { $B down --volumes --remove-orphans > /dev/null 2>&1; }
cli() { docker exec "$($B ps -q pbx)" asterisk -C /run/asterisk/config/asterisk.conf -rx "$1"; }
channels() { cli 'core show channels count' | sed -n 's/^\([0-9]*\) active channel.*/\1/p'; }
contacts() { cli 'pjsip show contacts' | grep -c 'Contact:.*1001'; }
container() { # name, extra docker arguments..., -- command...
    name=$1; shift
    docker run --name "$name" --label com.docker.compose.project=rust-microsip-lab \
        --network "$net" --user "$LAB_UID:$LAB_GID" --read-only --cap-drop ALL \
        --security-opt no-new-privileges --memory 2g --pids-limit 512 \
        --tmpfs /tmp:rw,mode=1777 "$@"
}
wait_phase() { # dir, phase, seconds
    timeout "$3" sh -c "until [ -e '$1/phase-$2' ]; do sleep 0.3; done"
}
report() { # scenario, result, observations (JSON object)
    python3 -I - "$1" "$2" "$3" "$commit" "$dirty" "$image_id" "$binaries" "$run/$1" <<'EOF'
import hashlib, json, os, sys
scenario, result, observations, commit, dirty, image, binaries, out = sys.argv[1:]
logs = {}
for name in sorted(os.listdir(out)):
    if name.endswith((".log", ".txt")):
        with open(os.path.join(out, name), "rb") as f:
            logs[name] = hashlib.sha256(f.read()).hexdigest()
row = {
    "scenario": scenario, "result": result, "observations": json.loads(observations),
    "commit": commit, "uncommitted_tracked_changes": int(dirty), "image": image,
    "binaries": {line.split()[1]: line.split()[0] for line in binaries.splitlines()},
    "files_sha256": logs,
}
with open(os.path.join(out, "report.json"), "w") as f:
    json.dump(row, f, indent=2)
print(f"{scenario}: {result}")
EOF
}

engine_lab() {
    out=$run/engine-lab; mkdir -p "$out"; pbx_up
    container rust-microsip-suite-engine --rm \
        --mount "type=bind,source=$PWD/$clients,target=/lab,readonly" \
        --entrypoint /usr/local/bin/engine-lab "$image" > "$out/engine-lab.log" 2>&1
    code=$?
    rows=$(grep -c '^{' "$out/engine-lab.log")
    pbx_down
    [ $code -eq 0 ] && result=pass || result=fail
    report engine-lab "$result" "{\"exit\": $(j "$code"), \"scenario_rows\": $(j "$rows")}"
}

engine_network() {
    out=$run/engine-network; mkdir -p "$out"; pbx_up
    name=rust-microsip-suite-network
    container $name -d \
        --mount "type=bind,source=$PWD/$clients,target=/lab,readonly" \
        --entrypoint /usr/local/bin/engine-network "$image" > /dev/null
    ip=$(docker inspect -f "{{(index .NetworkSettings.Networks \"$net\").IPAddress}}" $name)
    timeout 120 sh -c "until docker logs $name 2>&1 | grep -q renewal_window; do sleep 1; done"
    renewed=$(contacts)
    timeout 60 sh -c "until docker logs $name 2>&1 | grep -q in_call; do sleep 0.5; done"
    in_call=$(channels)
    docker network disconnect $net $name; sleep 40
    docker network connect --ip "$ip" $net $name
    timeout 400 sh -c "until docker logs $name 2>&1 | grep -q recovered; do sleep 1; done"; sleep 3
    recovered_contacts=$(contacts); recovered_channels=$(channels)
    code=$(docker wait $name); docker logs $name > "$out/engine-network.log" 2>&1; docker rm $name > /dev/null
    end_contacts=$(contacts)
    pbx_down
    result=fail
    [ "$code" = 0 ] && [ "$renewed" = 1 ] && [ "$in_call" = 1 ] && [ "$recovered_contacts" = 1 ] \
        && [ "$recovered_channels" = 0 ] && [ "$end_contacts" = 0 ] && result=pass
    report engine-network "$result" "{\"exit\": $(j "$code"), \"contact_after_renewal_window\": $(j "$renewed"), \"channels_in_call\": $(j "$in_call"), \"contact_after_recovery\": $(j "$recovered_contacts"), \"channels_after_recovery\": $(j "$recovered_channels"), \"contact_after_exit\": $(j "$end_contacts")}"
}

gui() { # scenario name
    gui_name=$1; out=$run/gui-$gui_name; mkdir -p "$out"
    container rust-microsip-suite-gui -d --env-file "$run/password.env" \
        -e HOME=/tmp/home -e XDG_RUNTIME_DIR=/tmp/xdg \
        -v "$PWD/$run/cfg:/cfg:ro" -v "$PWD/docker/asterisk/suite:/suite:ro" -v "$PWD/$out:/capture" \
        --entrypoint dbus-run-session "$image" -- sh /suite/gui.sh "$gui_name" > /dev/null
}
gui_finish() {
    wait_phase "$out" exited 240
    docker rm -f rust-microsip-suite-gui > /dev/null 2>&1
}
j() { echo "${1:-null}"; }
value() { sed -n "s/^\(.* \)\{0,1\}$1=\([^ ]*\).*/\2/p" "$out/results.txt" | tail -1; }
ack() { touch "$out/ack-$1"; }

gui_link() {
    pbx_up; gui link
    wait_phase "$out" registered 120; registered=$(contacts); ack registered
    wait_phase "$out" link-filled 60; before_enter=$(channels); ack link-filled
    wait_phase "$out" in-call 60; in_call=$(channels); ack in-call
    wait_phase "$out" ended 60; ended=$(channels); ack ended
    gui_finish; after_exit=$(contacts); pbx_down
    forward=$(value forward_exit); app_exit=$(value app_exit)
    result=fail
    [ "$registered" = 1 ] && [ "$forward" = 0 ] && [ "$before_enter" = 0 ] && [ "$in_call" = 1 ] \
        && [ "$ended" = 0 ] && [ "$app_exit" = 143 ] && [ "$after_exit" = 0 ] && result=pass
    report gui-link "$result" "{\"registered\": $(j "$registered"), \"second_launch_exit\": $(j "$forward"), \"channels_before_enter\": $(j "$before_enter"), \"channels_in_call\": $(j "$in_call"), \"channels_after_escape\": $(j "$ended"), \"app_exit\": $(j "$app_exit"), \"contact_after_exit\": $(j "$after_exit")}"
}

gui_frozen_exit() {
    pbx_up; gui frozen-exit
    wait_phase "$out" in-call 120; in_call=$(channels)
    docker pause "$($B ps -q pbx)" > /dev/null; touch "$out/pbx-frozen"
    gui_finish
    docker unpause "$($B ps -q pbx)" > /dev/null; pbx_down
    code=$(value frozen_exit); elapsed=$(value elapsed_ms)
    result=fail
    [ "$in_call" = 1 ] && [ "$code" = 143 ] && [ "${elapsed:-99999}" -lt 6000 ] \
        && ! grep -q left_running "$out/results.txt" && result=pass
    report gui-frozen-exit "$result" "{\"channels_in_call\": $(j "$in_call"), \"app_exit\": $(j "$code"), \"exit_ms_with_pbx_paused\": $(j "$elapsed")}"
}

gui_theme() {
    pbx_up; gui theme
    wait_phase "$out" dark-in-call 180; during=$(channels); ack dark-in-call
    gui_finish; pbx_down
    system=$(value system_theme_brightness); dark=$(value dark_in_call_brightness); again=$(value relaunched_brightness)
    result=fail
    [ "$during" = 1 ] && [ "${system:-0}" -gt 60 ] && [ "${dark:-100}" -lt 40 ] && [ "${again:-100}" -lt 40 ] && result=pass
    report gui-theme "$result" "{\"system_theme_without_portal_brightness_pct\": $(j "$system"), \"channels_after_switch\": $(j "$during"), \"dark_in_call_brightness_pct\": $(j "$dark"), \"relaunched_brightness_pct\": $(j "$again")}"
}

gui_a11y() {
    pbx_up; gui a11y; gui_finish; pbx_down
    unnamed=$(sed -n 's/^UNNAMED_TOTAL //p' "$out/screens.txt")
    wraps=$(grep -c "'Menu'" "$out/focus.txt")
    result=fail
    [ "$(value screens_exit)" = 0 ] && [ "$unnamed" = 0 ] && [ "$wraps" -ge 1 ] && result=pass
    report gui-a11y "$result" "{\"screens\": $(grep -c '^==' "$out/screens.txt"), \"unnamed\": ${unnamed:-null}, \"tab_cycle_reached_menu\": $wraps}"
}

gui_a11y_call() {
    pbx_up; gui a11y-call
    wait_phase "$out" in-call 120; in_call=$(channels); ack in-call
    wait_phase "$out" ended 120; ended=$(channels); ack ended
    gui_finish; pbx_down
    reached=0
    for control in Call End XFER HOLD MUTE; do
        grep -q "'$control'" "$out/focus.txt" && reached=$((reached + 1))
    done
    result=fail
    [ "$in_call" = 1 ] && [ "$ended" = 0 ] && [ "$(value call_sweep_exit)" = 0 ] \
        && [ "$(value transfer_sweep_exit)" = 0 ] && [ $reached = 5 ] && result=pass
    report gui-a11y-call "$result" "{\"channels_in_call\": $in_call, \"channels_after_end\": $ended, \"call_controls_reached_by_tab\": $reached}"
}

trap 'docker rm -f rust-microsip-suite-gui rust-microsip-suite-network > /dev/null 2>&1; pbx_down' EXIT
for scenario in $scenarios; do
    case $scenario in
        engine-lab) engine_lab ;;
        engine-network) engine_network ;;
        gui-link) gui_link ;;
        gui-frozen-exit) gui_frozen_exit ;;
        gui-theme) gui_theme ;;
        gui-a11y) gui_a11y ;;
        gui-a11y-call) gui_a11y_call ;;
        *) echo "unknown scenario: $scenario" >&2; exit 2 ;;
    esac
done
rm -rf "$run/cfg" "$run/password.env"
python3 -I - "$run" <<'EOF'
import json, os, sys
run = sys.argv[1]
rows = [json.load(open(os.path.join(run, d, "report.json")))
        for d in sorted(os.listdir(run)) if os.path.isfile(os.path.join(run, d, "report.json"))]
summary = {"run": os.path.basename(run), "commit": rows[0]["commit"] if rows else None,
           "passed": sum(r["result"] == "pass" for r in rows), "total": len(rows),
           "scenarios": {r["scenario"]: r["result"] for r in rows}}
json.dump(summary, open(os.path.join(run, "summary.json"), "w"), indent=2)
print(json.dumps(summary))
EOF
echo "leftover=$(docker ps -aq --filter label=com.docker.compose.project=rust-microsip-lab | wc -l)"
grep -q '"result": "fail"' "$run"/*/report.json && exit 1
exit 0
