#!/usr/bin/env bash
# Copies Horizon to a Nintendo Switch's SD card mounted on this computer (hekate:
# Tools > USB Tools > SD Card), as docs/switch.md lays it out:
#   sdmc:/switch/horizon/horizon.nro            the game
#   sdmc:/switch/horizon/resources/             .local/horizon-client/26.3 (horizon-prepare)
#   sdmc:/switch/horizon-smoke/horizon-smoke.nro  the std smoke test, when built
# Usage: switch/install-sd.sh /run/media/$USER/SWITCH-SD
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SD=${1:?usage: switch/install-sd.sh SD_CARD_MOUNT_POINT}
[ -d "$SD" ] || { echo "not a directory: $SD" >&2; exit 1; }
[ -d "$SD/switch" ] || [ -d "$SD/atmosphere" ] || { echo "$SD does not look like a Switch SD card (no switch/ or atmosphere/)" >&2; exit 1; }
NRO=$ROOT/target/switch/horizon.nro
RES=$ROOT/.local/horizon-client/26.3
[ -f "$NRO" ] || { echo "build it first: switch/build.sh" >&2; exit 1; }
mkdir -p "$SD/switch/horizon"
echo "game -> $SD/switch/horizon/horizon.nro"
cp "$NRO" "$SD/switch/horizon/horizon.nro"
if [ -f "$ROOT/target/switch/horizon-smoke.nro" ]; then
    mkdir -p "$SD/switch/horizon-smoke"
    cp "$ROOT/target/switch/horizon-smoke.nro" "$SD/switch/horizon-smoke/horizon-smoke.nro"
fi
if [ -f "$RES/horizon-resources.json" ]; then
    echo "resources -> $SD/switch/horizon/resources ($(du -sh "$RES" | cut -f1))"
    # FAT32/exFAT: no owners or permissions; sizes and times decide what to copy again.
    rsync -r -t --modify-window=2 --delete --info=progress2 "$RES/" "$SD/switch/horizon/resources/"
else
    echo "no prepared resources in $RES (horizon-prepare --prepare --complete); copied the game only" >&2
fi
echo "flushing to the card..."
sync
echo "done: eject the card, then start the Homebrew Menu holding R on a game and pick Horizon"
