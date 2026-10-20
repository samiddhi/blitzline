#!/bin/sh
# Opens native windows briefly and captures them; requires a desktop session.
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" = Darwin ]; then
    exec sh distribution/native-visual-check.sh
fi
cargo build --locked -p blitzline-desktop
mkdir -p target/previews
for appearance in light dark; do
for scenario in projects setup assistant cards export; do
    EFRAME_SCREENSHOT_TO="$PWD/target/previews/$scenario-$appearance.png" \
        target/debug/blitzline-desktop \
        --data-dir "$PWD/target/visual-$scenario" --scenario "$scenario" --narrow --appearance "$appearance"
done
done
echo "Previews saved in target/previews/"
