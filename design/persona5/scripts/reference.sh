#!/usr/bin/env bash
# Renders the reference set of the settled designs into reference/: stills of each window and
# component sheet, and short clips of each element moving. Takes about 25 minutes; a pattern
# renders only the matching files.
#
#   design/persona5/scripts/reference.sh
#   design/persona5/scripts/reference.sh '*-progress*'
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release
bin=target/release/persona5-sketches
out=reference
only=${1:-*}
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

still() { # name scene
  [[ $1 == $only ]] || return 0
  $bin --snap "$out/$1.png" --scene "$2" --size 960x600 --scale 2 --at 2 >/dev/null
}

# clip name scene script duration fps crop [start-frame]
# The clip starts at 2 s (after the window appears) unless a start frame is given.
clip() {
  local name=$1 scene=$2 script=$3 dur=$4 fps=$5 crop=$6 start=${7:-$(($5 * 2))}
  [[ $name == $only ]] || return 0
  local f="$tmp/$name"
  mkdir -p "$f"
  $bin --scene "$scene" --script "scripts/$script.json" --out "$f" --duration "$dur" --fps "$fps" \
    --size 960x600 --scale 2 >/dev/null
  ffmpeg -loglevel error -y -framerate "$fps" -start_number "$start" -i "$f/frame_%04d.png" \
    -vf "$crop" -c:v libx264 -pix_fmt yuv420p -crf 23 -movflags +faststart "$out/$name.mp4"
  rm -rf "$f"
}

for theme in phantom-thieves jose-shop; do
  still "$theme" "$theme"
  still "$theme-parts" "$theme-parts"
  # The whole window at rest, at half size: the ground and everything idling.
  clip "$theme-window" "$theme" empty 12 30 "scale=960:-2"
  # The toolbar and the list in use (hover, press, select), at 60 fps.
  clip "$theme-toolbar" "$theme" walk 12.5 60 "crop=1920:200:0:0"
  clip "$theme-list" "$theme" select 9 60 "crop=1920:330:0:240"
  # A torrent pausing itself under a hovered Pause: hovered to disabled and back.
  clip "$theme-states" "$theme-status-flip" hover-pause 8 60 "crop=900:220:1000:0"
  # Rows added and removed.
  clip "$theme-row-added" "$theme-rows-add" empty 5 60 "crop=1920:560:0:220"
  clip "$theme-row-removed" "$theme-rows-remove" empty 5 60 "crop=1920:560:0:220"
  # Progress reports smoothed (steady updates, a jump, a drop), and every bar 0 to 100 and back.
  clip "$theme-progress" "$theme-progress" empty 28 60 "crop=1040:500:560:230"
  clip "$theme-progress-sweep" "$theme-progress-sweep" empty 15 60 "crop=1040:500:560:230"
  clip "$theme-scroll" "$theme-scroll" empty 12 30 "crop=1920:940:0:160,scale=960:-2"
done
# Jose's nebula cross-fades over 90 s: a 10x time-lapse of 100 s, at half size.
clip jose-shop-sky-10x jose-shop empty 102 3 "scale=960:-2" 6
ls -la "$out"
