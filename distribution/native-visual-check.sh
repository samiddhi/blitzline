#!/bin/sh
# Captures only the prototype window, including the AppKit toolbar.
# Requires macOS Screen Recording permission for the invoking application.
set -eu
cd "$(dirname "$0")/.."
cargo build --locked -p blitzline-desktop
mkdir -p target/previews
run_dir=$(mktemp -d "$PWD/target/native-preview.XXXXXX")
child=""
trap 'if [ -n "$child" ]; then kill "$child" 2>/dev/null || true; fi' EXIT
if [ "$#" -eq 0 ]; then
    set -- projects setup assistant cards export
fi
for appearance in light dark; do
for scenario in "$@"; do
    case "$scenario" in
        help) capture_title="Blitzline Help" ;;
        settings) capture_title="Blitzline settings" ;;
        project-settings) capture_title="Blitzline · Project settings" ;;
        chat-window) capture_title="Blitzline · Chat assistant" ;;
        *) capture_title="Blitzline" ;;
    esac
    BLITZLINE_CAPTURE_WINDOW_TITLE="$capture_title" BLITZLINE_NATIVE_INSPECT=1 target/debug/blitzline-desktop \
        --data-dir "$run_dir/$scenario-$appearance" \
        --scenario "$scenario" --appearance "$appearance" --narrow \
        >"target/previews/$scenario-$appearance.log" 2>&1 &
    child=$!
    sleep 3
    window_id=$(sed -n 's/^native-capture-id=//p' "target/previews/$scenario-$appearance.log" | tail -1)
    test -n "$window_id"
    screencapture -x -l "$window_id" "target/previews/$scenario-$appearance.png"
    kill "$child"
    wait "$child" 2>/dev/null || true
    child=""
done
done
