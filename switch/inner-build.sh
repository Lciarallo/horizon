#!/usr/bin/env bash
# Runs inside the build container (switch/build.sh). /work is the repository.
set -euo pipefail
cd /work
TARGET=/work/switch/aarch64-nintendo-switch-newlib.json
export CARGO_TARGET_DIR=/work/target/switch
PROFILE=${HORIZON_SWITCH_PROFILE:-release}
OUT=$CARGO_TARGET_DIR/aarch64-nintendo-switch-newlib/$PROFILE
# getrandom 0.3/0.4 only know the 3DS among `horizon` targets: their custom backend calls
# __getrandom_v03_custom, which horizon-switch defines (libnx's csrng).
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"custom\""
# C code in dependencies (ring, zstd...) is compiled by devkitA64's GCC.
SWITCH_CFLAGS="-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE -ffunction-sections -fdata-sections -D__SWITCH__ -I$DEVKITPRO/libnx/include -I$DEVKITPRO/portlibs/switch/include"
export CC_aarch64_nintendo_switch_newlib=aarch64-none-elf-gcc
export CXX_aarch64_nintendo_switch_newlib=aarch64-none-elf-g++
export AR_aarch64_nintendo_switch_newlib=aarch64-none-elf-ar
export CFLAGS_aarch64_nintendo_switch_newlib="$SWITCH_CFLAGS"
export CXXFLAGS_aarch64_nintendo_switch_newlib="$SWITCH_CFLAGS"
# The Switch's release: whole-program LTO (slower to build, a few percent faster to run).
if [ "$PROFILE" = release ]; then export CARGO_PROFILE_RELEASE_LTO=fat; fi
CARGO_FLAGS=(-Zjson-target-spec -Zbuild-std=std,panic_unwind --target "$TARGET" --profile "$PROFILE")

nro() { # elf name title icon
    local elf=$1 name=$2 title=$3 icon=$4
    nacptool --create "$title" "Horizon contributors" "$(cat /work/switch/VERSION)" "$OUT/$name.nacp"
    elf2nro "$OUT/$elf" "$CARGO_TARGET_DIR/$name.nro" --icon="$icon" --nacp="$OUT/$name.nacp"
    ls -la "$CARGO_TARGET_DIR/$name.nro"
}

case "$1" in
smoke)
    (cd switch/smoke && cargo build "${CARGO_FLAGS[@]}" "${@:2}")
    nro horizon-smoke horizon-smoke "Horizon smoke test" /work/switch/nro/icon.jpg
    ;;
game)
    cargo build -p horizon-client --no-default-features --features switch "${CARGO_FLAGS[@]}" "${@:2}"
    # The grass block switch/tools/make-icon.py draws from the player's own resources, if made.
    icon=/work/switch/nro/icon.jpg
    [ -f /work/.local/switch/icon.jpg ] && icon=/work/.local/switch/icon.jpg
    nro horizon-client horizon "Horizon" "$icon"
    ;;
check)
    cargo check --keep-going -p horizon-client --no-default-features --features switch "${CARGO_FLAGS[@]}" "${@:2}"
    ;;
shell)
    exec bash
    ;;
*)
    echo "usage: switch/build.sh [game|smoke|check|shell]" >&2
    exit 2
    ;;
esac
