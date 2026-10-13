#!/bin/sh
# One GUI scenario of the lab suite, inside the suite-runner container:
# private Xvfb, private D-Bus session with AT-SPI, the release app, and a
# fictional lab account. Progress is signalled to the host through
# /capture/phase-* files; the host checks the PBX at each phase.
# Usage (by suite.sh): gui.sh link|frozen-exit|theme|a11y|a11y-call|control
set -u
umask 077
scenario=$1
mkdir -p /tmp/home /tmp/xdg && chmod 700 /tmp/xdg
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg DISPLAY=:99
export WINIT_UNIX_BACKEND=x11 WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe
unset WAYLAND_DISPLAY
app_bin=/usr/local/bin/rust-microsip
tool="python3 -I /suite/atspi.py"

Xvfb :99 -screen 0 800x768x24 -dpi 96 -nolisten tcp -noreset > /capture/xvfb.log 2>&1 &
timeout 20 sh -c 'until xdotool getdisplaygeometry >/dev/null 2>&1; do sleep 0.1; done'
/usr/libexec/at-spi-bus-launcher --launch-immediately > /capture/atspi.log 2>&1 &
sleep 2
for property in IsEnabled ScreenReaderEnabled; do
    dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
        org.freedesktop.DBus.Properties.Set string:org.a11y.Status \
        "string:$property" variant:boolean:true > /dev/null 2>&1
done
# Start the registry before the app so AccessKit finds it.
python3 -I -c 'import gi; gi.require_version("Atspi", "2.0"); from gi.repository import Atspi; Atspi.get_desktop(0)' > /dev/null 2>&1

phase() { touch "/capture/phase-$1"; }
# A phase the host inspects: wait until it has checked the PBX.
checkpoint() {
    phase "$1"
    timeout 60 sh -c "until [ -e /capture/ack-$1 ]; do sleep 0.2; done" \
        || echo "no host check at $1" >> /capture/failure
}
shot() { xwd -id "$window" -silent -out /tmp/shot.xwd && convert /tmp/shot.xwd "/capture/$1.png"; }
# Mean brightness of a capture in percent (theme check).
brightness() { convert "/capture/$1.png" -colorspace Gray -format '%[fx:int(100*mean)]' info:; }
start_app() { # log name, extra arguments
    log=$1; shift
    "$app_bin" --config /cfg/config.toml "$@" > "/capture/$log.log" 2>&1 &
    app=$!
    window=$(timeout 45 xdotool search --sync --onlyvisible --name rust-microsip | tail -1)
    [ -n "$window" ] || { echo "window did not appear" > /capture/failure; phase exited; exit 1; }
    sleep 6
    xdotool windowactivate --sync "$window" 2>/dev/null
    xdotool windowfocus --sync "$window"
}
stop_app() { # label
    started=$(date +%s%N)
    kill -TERM "$app"; wait "$app"; code=$?
    echo "$1_exit=$code elapsed_ms=$(( ($(date +%s%N) - started) / 1000000 ))" >> /capture/results.txt
}
dial() { xdotool type --delay 80 "$1"; xdotool key Return; sleep 6; }
tabs() { # label, count
    for n in $(seq "$2"); do
        xdotool key Tab; sleep 0.4
        echo "$1$n $($tool focused)" >> /capture/focus.txt
    done
}

case "$scenario" in
    link) # LIN-02/03: a second launch forwards a link that only fills the number.
        start_app app; shot registered; checkpoint registered
        "$app_bin" --config /cfg/config.toml sip:602@pbx > /capture/forward.log 2>&1
        echo "forward_exit=$?" >> /capture/results.txt
        sleep 2; shot link-filled; checkpoint link-filled
        xdotool key Return; sleep 6; shot in-call; checkpoint in-call
        xdotool key Escape; sleep 3; shot ended; checkpoint ended
        stop_app app ;;
    frozen-exit) # LIN-04: SIGTERM during a call while the PBX is paused.
        start_app app; dial 600; shot in-call; phase in-call
        timeout 60 sh -c 'until [ -e /capture/pbx-frozen ]; do sleep 0.2; done'
        stop_app frozen
        pgrep -x rust-microsip > /dev/null && echo "left_running=1" >> /capture/results.txt ;;
    theme) # ND-03/04: dark theme from Settings during a call, then persistence.
        start_app app; shot system-theme
        echo "system_theme_brightness=$(brightness system-theme)" >> /capture/results.txt
        dial 600
        $tool press Menu "Settings…" Dark >> /capture/atspi-steps.txt
        xdotool key Escape; sleep 2; shot dark-in-call; checkpoint dark-in-call
        echo "dark_in_call_brightness=$(brightness dark-in-call)" >> /capture/results.txt
        sleep 2; stop_app app
        start_app relaunch; shot relaunched; phase relaunched
        echo "relaunched_brightness=$(brightness relaunched)" >> /capture/results.txt
        stop_app relaunch ;;
    a11y) # ND-05: every screen named; Tab cycle without traps.
        start_app app
        $tool screens > /capture/screens.txt; echo "screens_exit=$?" >> /capture/results.txt
        stop_app app
        start_app focus; tabs tab 34; xdotool key shift+Tab; sleep 0.4
        echo "shift-tab $($tool focused)" >> /capture/focus.txt
        stop_app focus ;;
    a11y-call) # ND-05 during a call: call controls named and reachable.
        start_app app
        xdotool type --delay 80 600; sleep 1; tabs pre 18
        xdotool key Return; sleep 6; checkpoint in-call
        tabs call 24
        $tool sweep "In call" End > /capture/screens.txt; echo "call_sweep_exit=$?" >> /capture/results.txt
        $tool press XFER >> /capture/atspi-steps.txt
        $tool sweep "Transfer" "Transfer to" >> /capture/screens.txt; echo "transfer_sweep_exit=$?" >> /capture/results.txt
        $tool press Cancel End >> /capture/atspi-steps.txt; sleep 2; checkpoint ended
        stop_app app ;;
    control) # AG-01/02/04/05: CLI clients against the window as owner.
        start_app app
        cli() { "$app_bin" "$@" --json 2>>/capture/cli-stderr.log; }
        # The prompt arms its buttons one second after it appears.
        decide() { sleep 1.5; $tool press "$1" >> /capture/atspi-steps.txt; }
        field() { python3 -I -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1], {"d": d}))' "$1"; }
        # Two clients at once read coherent status (no personal data).
        cli status > /capture/status-a.json & first=$!
        cli status > /capture/status-b.json; wait "$first"
        echo "registration=$(field 'd["data"]["registration"]' < /capture/status-a.json)" >> /capture/results.txt
        echo "registration_b=$(field 'd["data"]["registration"]' < /capture/status-b.json)" >> /capture/results.txt
        cli call dial 600 --key suite-dial > /capture/dial.json; echo "dial_exit=$?" >> /capture/results.txt
        op=$(field 'd["operation_id"]' < /capture/dial.json)
        echo "dial_status=$(field 'd["status"]' < /capture/dial.json)" >> /capture/results.txt
        sleep 2; echo "prompt_focus=$($tool focused | tr -d "' ")" >> /capture/results.txt
        checkpoint requested
        decide "Allow request"; sleep 4
        checkpoint dialed
        cli operation get "$op" > /capture/op.json
        echo "op_state=$(field 'd["data"]["state"]' < /capture/op.json)" >> /capture/results.txt
        # Same key again: reported, never repeated. Same key, other payload: conflict.
        cli call dial 600 --key suite-dial > /capture/replay.json; echo "replay_exit=$?" >> /capture/results.txt
        cli call dial 602 --key suite-dial > /capture/conflict.json
        echo "conflict=$(field 'd["error"]["code"]' < /capture/conflict.json)" >> /capture/results.txt
        sleep 2; checkpoint replayed
        cli status > /capture/status-call.json
        call_id=$(field 'd["data"]["call"]["call_id"]' < /capture/status-call.json)
        version=$(field 'd["data"]["call"]["version"]' < /capture/status-call.json)
        echo "status_has_remote=$(field '"remote" in d["data"]["call"] or "600" in str(d["data"]["call"])' < /capture/status-call.json)" >> /capture/results.txt
        # A stale version is refused before anything is recorded.
        cli call hangup "$call_id" 0 --key suite-stale > /capture/stale.json
        echo "stale=$(field 'd["error"]["code"]' < /capture/stale.json)" >> /capture/results.txt
        cli call hangup "$call_id" "$version" --key suite-hangup > /capture/hangup.json
        sleep 1; checkpoint hangup-requested
        decide "Allow request"; sleep 3
        checkpoint hung-up
        # Denied: no effect.
        cli dnd on --key suite-dnd > /capture/dnd.json
        dnd_op=$(field 'd["operation_id"]' < /capture/dnd.json)
        decide "Deny request"; sleep 1
        cli operation get "$dnd_op" > /capture/dnd-op.json
        echo "dnd_state=$(field 'd["data"]["state"]' < /capture/dnd-op.json)" >> /capture/results.txt
        cli status > /capture/status-end.json
        echo "dnd_after=$(field 'd["data"]["dnd"]' < /capture/status-end.json)" >> /capture/results.txt
        # A denied dial never reaches the PBX.
        cli call dial 600 --key suite-denied > /capture/denied.json
        sleep 2; decide "Deny request"; sleep 2
        checkpoint dial-denied
        # Personal reads need a grant.
        cli history list > /capture/history.json; echo "history_exit=$?" >> /capture/results.txt
        # MCP through the official SDK client: dial, approve, replay, disconnect.
        /usr/local/bin/mcp-probe "$app_bin" 600 mcp-dial > /capture/mcp.jsonl 2> /capture/mcp-stderr.log &
        probe=$!
        timeout 30 sh -c 'until grep -q "\"step\":\"dial\"" /capture/mcp.jsonl; do sleep 0.2; done'
        sleep 1; checkpoint mcp-requested
        decide "Allow request"
        wait "$probe"; echo "mcp_exit=$?" >> /capture/results.txt
        line() { grep "\"step\":\"$1\"" /capture/mcp.jsonl; }
        echo "mcp_tools=$(line tools | field 'd["count"]')" >> /capture/results.txt
        echo "mcp_dial=$(line dial | field 'd["status"]')" >> /capture/results.txt
        echo "mcp_state=$(line approved | field 'd["state"]')" >> /capture/results.txt
        echo "mcp_replay=$(line replay | field 'd["status"]')" >> /capture/results.txt
        echo "mcp_same=$(line replay | field 'd["same_operation"]')" >> /capture/results.txt
        checkpoint mcp-disconnected
        # The call stays with the owner after the client left; end it from the CLI.
        cli status > /capture/status-mcp.json
        call_id=$(field 'd["data"]["call"]["call_id"]' < /capture/status-mcp.json)
        version=$(field 'd["data"]["call"]["version"]' < /capture/status-mcp.json)
        cli call hangup "$call_id" "$version" --key mcp-cleanup > /capture/mcp-hangup.json
        decide "Allow request"; sleep 3
        checkpoint mcp-ended
        stop_app app
        # No owner: unavailable, nothing started.
        cli status > /capture/status-none.json; echo "no_owner_exit=$?" >> /capture/results.txt
        pgrep -x rust-microsip > /dev/null && echo "spawned=1" >> /capture/results.txt ;;
    grants) # AG-04: grants issued in the window, then the headless service.
        start_app app
        cli() { "$app_bin" "$@" --json 2>>/capture/cli-stderr.log; }
        field() { python3 -I -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1], {"d": d}))' "$1"; }
        decide() { sleep 1.5; $tool press "$1" >> /capture/atspi-steps.txt; }
        result() { echo "$1=$2" >> /capture/results.txt; }
        call_ref() { cli status $1 > /capture/st.json; echo "$(field 'd["data"]["call"]["call_id"]' < /capture/st.json) $(field 'd["data"]["call"]["version"]' < /capture/st.json)"; }
        # A person issues a grant in Settings → Agent access.
        $tool press Menu "Settings…" >> /capture/atspi-steps.txt
        for scope in control:dial control:hangup read:history; do $tool press "$scope" >> /capture/atspi-steps.txt; done
        $tool focus "May call" >> /capture/atspi-steps.txt; xdotool type --delay 60 600; sleep 1
        $tool press "Create grant file" >> /capture/atspi-steps.txt; sleep 1
        grant=$(ls /tmp/xdg/rust-microsip/grants/*.grant 2>/dev/null | head -1)
        result grant_mode "$(stat -c %a "$grant" 2>/dev/null)"
        $tool press Cancel >> /capture/atspi-steps.txt
        G="--grant-file $grant"
        cli history list $G > /capture/history.json; result history_exit $?
        # Outside the grant (another number): a person decides; denied.
        cli call dial 602 --key gr-602 $G > /capture/d602.json; result d602 "$(field 'd["status"]' < /capture/d602.json)"
        decide "Deny request"; sleep 1; checkpoint denied-602
        # Inside the grant: runs without a prompt.
        cli call dial 600 --key gr-600 $G > /capture/d600.json; result d600 "$(field 'd["status"]' < /capture/d600.json)"
        sleep 5; result pending_after_grant "$($tool focused | tr -d "' ")"
        checkpoint granted-dial
        set -- $(call_ref "$G")
        cli call hangup "$1" "$2" --key gr-hangup $G > /capture/hangup.json; result hangup "$(field 'd["status"]' < /capture/hangup.json)"
        sleep 3; checkpoint granted-hangup
        # Quota of one call: the next dial needs a person again.
        cli call dial 600 --key gr-600b $G > /capture/d600b.json; result quota "$(field 'd["status"]' < /capture/d600b.json)"
        decide "Deny request"
        # Revoked: nothing works any more.
        $tool press Menu "Settings…" "Revoke grant" >> /capture/atspi-steps.txt
        cli history list $G > /capture/history2.json; result revoked_exit $?
        # Export a single-use bootstrap (same ticks and number) for the service.
        $tool press "Export for headless service" >> /capture/atspi-steps.txt; sleep 1
        boot=$(ls /tmp/home/.config/rust-microsip/bootstrap/*.json 2>/dev/null | head -1)
        result boot_mode "$(stat -c %a "$boot" 2>/dev/null)"
        $tool press Cancel >> /capture/atspi-steps.txt
        stop_app app
        # The headless service: no display needed, only what was granted.
        DISPLAY= "$app_bin" service --grant-file "$boot" --config /cfg/config.toml > /capture/service.log 2>&1 &
        svc=$!
        timeout 30 sh -c 'until grep -q "service ready" /capture/service.log; do sleep 0.3; done'
        sleep 6
        "$app_bin" --config /cfg/config.toml > /capture/gui-vs-service.log 2>&1; result gui_vs_service $?
        B="--grant-file $boot"
        cli status $B > /capture/svc-status.json
        result svc_mode "$(field 'd["data"]["mode"]' < /capture/svc-status.json)"
        result svc_registration "$(field 'd["data"]["registration"]' < /capture/svc-status.json)"
        cli call dial 602 --key sv-602 $B > /capture/sv602.json; result sv602 "$(field 'd["error"]["code"]' < /capture/sv602.json)"
        cli call dial 600 --key sv-600 $B > /capture/sv600.json; result sv600 "$(field 'd["status"]' < /capture/sv600.json)"
        sleep 5; checkpoint service-dial
        set -- $(call_ref "$B")
        cli call hangup "$1" "$2" --key sv-hangup $B > /capture/svh.json; result sv_hangup "$(field 'd["status"]' < /capture/svh.json)"
        sleep 3; checkpoint service-hangup
        kill -TERM "$svc"; wait "$svc"; result service_exit $?
        grep -q "service stopped" /capture/service.log && result service_stopped 1
        grep -q "microphone active" /capture/service.log && result microphone_reported 1
        grep -q "microphone released" /capture/service.log && result microphone_released 1
        # Single use: the same bootstrap never starts another session.
        DISPLAY= "$app_bin" service --grant-file "$boot" --config /cfg/config.toml > /capture/service2.log 2>&1
        result reuse_exit $?
        grep -q bootstrap_consumed /capture/service2.log && result reuse_consumed 1
        # The capabilities never appear in logs, outputs or the database.
        leaks=0
        for cap in "$(tr -d '\n' < "$grant")" "$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["capability"])' "$boot")"; do
            grep -rlF "$cap" /capture /tmp/home/.local/share 2>/dev/null && leaks=$((leaks + 1))
        done
        result capability_leaks "$leaks" ;;
    *) echo "unknown scenario $scenario" > /capture/failure ;;
esac
phase exited
