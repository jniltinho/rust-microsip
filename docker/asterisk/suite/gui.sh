#!/bin/sh
# One GUI scenario of the lab suite, inside the suite-runner container:
# private Xvfb, private D-Bus session with AT-SPI, the release app, and a
# fictional lab account. Progress is signalled to the host through
# /capture/phase-* files; the host checks the PBX at each phase.
# Usage (by suite.sh): gui.sh link|frozen-exit|theme|a11y|a11y-call
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
checkpoint() { phase "$1"; timeout 60 sh -c "until [ -e /capture/ack-$1 ]; do sleep 0.2; done"; }
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
        $tool sweep "In call" > /capture/screens.txt; echo "call_sweep_exit=$?" >> /capture/results.txt
        $tool press XFER >> /capture/atspi-steps.txt
        $tool sweep "Transfer" >> /capture/screens.txt; echo "transfer_sweep_exit=$?" >> /capture/results.txt
        $tool press Cancel End >> /capture/atspi-steps.txt; sleep 2; checkpoint ended
        stop_app app ;;
    *) echo "unknown scenario $scenario" > /capture/failure ;;
esac
phase exited
