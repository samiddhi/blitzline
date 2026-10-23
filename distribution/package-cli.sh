#!/bin/sh
# Build a standalone native CLI archive. Requires Rust only on the build machine.
set -eu
cd "$(dirname "$0")/.."
profile=release
if [ "${1:-}" = "--debug" ]; then
    profile=debug
    cargo build --locked -p blitzline-cli --bin blitzline
else
    cargo build --locked --release -p blitzline-cli --bin blitzline
fi
platform=$(uname -s | tr '[:upper:]' '[:lower:]')
architecture=$(uname -m)
package="target/dist/blitzline-$platform-$architecture"
mkdir -p "$package"
cp "target/$profile/blitzline" "$package/blitzline"
cp distribution/CLI-README.md "$package/README.md"
tar -czf "$package.tar.gz" -C "target/dist" "blitzline-$platform-$architecture"
echo "Ready: $package.tar.gz"
