#!/usr/bin/env bash
# Builds Horizon for the Nintendo Switch (homebrew .nro) inside the devkitPro + Rust
# nightly container (packaging/switch/Dockerfile). Needs podman or docker.
#
#   switch/build.sh            the game: target/switch/horizon.nro
#   switch/build.sh smoke      the std smoke test: target/switch/horizon-smoke.nro
#   switch/build.sh shell      a shell in the build container
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=${HORIZON_SWITCH_IMAGE:-horizon-switch-build}
ENGINE=${CONTAINER_ENGINE:-$(command -v podman || command -v docker)}
if ! "$ENGINE" image inspect "$IMAGE" >/dev/null 2>&1; then
    "$ENGINE" build -t "$IMAGE" "$ROOT/packaging/switch"
fi
TTY=()
[ -t 0 ] && TTY=(-it)
exec "$ENGINE" run --rm "${TTY[@]}" \
    -v "$ROOT":/work:z \
    -v horizon-switch-cargo:/opt/cargo/registry \
    -w /work \
    -e CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-auto}" \
    -e HORIZON_SWITCH_PROFILE="${HORIZON_SWITCH_PROFILE:-release}" \
    "$IMAGE" /work/switch/inner-build.sh "${1:-game}" "${@:2}"
