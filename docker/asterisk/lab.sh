#!/bin/sh
# Only disposable lab paths are mounted; never HOME, base/, DBus or Docker socket.
set -eu
cd "$(dirname "$0")/../.."
export LAB_UID="$(id -u)" LAB_GID="$(id -g)"
if [ "$LAB_UID" -eq 0 ]; then echo "ERROR: run the lab as a non-root user." >&2; exit 1; fi
test ! -L artifacts
image=rust-microsip-asterisk:22.11.0-lab.1
compose() { docker compose -f docker/asterisk/compose.yaml "$@"; }
host_compose() { docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.host.yaml "$@"; }
case "${1:-}" in
    build) docker buildx build --platform linux/amd64 --target runtime --load -t "$image" docker/asterisk ;;
    init)
        test ! -L artifacts && test ! -L artifacts/asterisk-lab
        mkdir -p -m 700 artifacts/asterisk-lab
        docker run --rm --network none --user "$LAB_UID:$LAB_GID" --read-only \
            --cap-drop ALL --security-opt no-new-privileges \
            --tmpfs /tmp:rw,noexec,nosuid,nodev,mode=1777,size=32m \
            --mount "type=bind,source=$(pwd)/artifacts/asterisk-lab,target=/lab" \
            "$image" generate ;;
    unit-test)
        docker run --rm --network none --user "$LAB_UID:$LAB_GID" --read-only \
            --cap-drop ALL --security-opt no-new-privileges \
            --tmpfs /tmp:rw,noexec,nosuid,nodev,mode=1777,size=32m "$image" unit-test ;;
    test)
        cleanup() {
            if [ ! -L artifacts/asterisk-smoke.log ]; then
                compose logs --no-color > artifacts/asterisk-smoke.log 2>&1 || :
            fi
            compose down --volumes --remove-orphans
        }
        trap cleanup EXIT
        trap 'exit 130' INT
        trap 'exit 143' TERM
        timeout --signal=TERM --kill-after=20s 120s docker compose -f docker/asterisk/compose.yaml up --no-build --abort-on-container-exit --exit-code-from test-runner ;;
    up) compose up --no-build -d --wait pbx peer ;;
    down) compose down --volumes --remove-orphans ;;
    host-up) host_compose up --no-build -d --wait pbx ;;
    host-down) host_compose down --volumes --remove-orphans ;;
    config-check)
        compose config --format json | python3 docker/asterisk/verify_compose.py internal
        host_compose config --format json | python3 docker/asterisk/verify_compose.py host ;;
    *) printf '%s\n' 'Usage: lab.sh build|init|unit-test|config-check|test|up|down|host-up|host-down' >&2; exit 2 ;;
esac
