#!/usr/bin/env bash
# Renders every direction headless into out/: the window still, the component sheet, and a GIF
# of hovering and pressing buttons and selecting rows (scripts/interact.json).
#
#   design/persona5/scripts/render.sh [card|accent|attack]...
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q
bin=target/debug/persona5-sketches
mkdir -p out
dirs=("$@")
[ ${#dirs[@]} -gt 0 ] || dirs=(card accent attack)
for d in "${dirs[@]}"; do
  # --at lets the appear motion finish before the still is taken.
  $bin --snap "out/$d.png" --scene "$d" --size 960x600 --scale 2 --theme dark --at 1.5 >/dev/null
  $bin --snap "out/$d-parts.png" --scene "$d-parts" --size 960x620 --scale 2 --theme dark --at 1.5 >/dev/null
  frames=$(mktemp -d)
  $bin --scene "$d" --script scripts/interact.json --out "$frames" --duration 7 --fps 30 \
    --size 960x600 --scale 2 --theme dark --keys --pointer >/dev/null
  ffmpeg -loglevel error -y -framerate 30 -i "$frames/frame_%04d.png" \
    -vf "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" "out/$d.gif"
  rm -rf "$frames"
done
ls -la out
