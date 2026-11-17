#!/usr/bin/env bash
# Builds the release sketch and opens it in a window to click around: the primary button reads
# Switch and swaps Phantom Thieves and Jose's shop.
#
#   design/persona5/scripts/play.sh
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release
exec target/release/persona5-sketches --scene play
