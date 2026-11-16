#!/bin/sh
# Prints `name value` for every fact in abi_dump.c, as devkitA64 + libnx define them.
set -e
cd "$(dirname "$0")"
aarch64-none-elf-gcc -S -O0 -D__SWITCH__ -I"$DEVKITPRO/libnx/include" -march=armv8-a+crc+crypto -o - abi_dump.c \
  | awk '/^abi_/ { name = $1; sub(/:$/, "", name); sub(/^abi_/, "", name) } /\.(x|d)?word|\.quad/ && name { print name, $2; name = "" } /\.zero/ && name { print name, 0; name = "" }'
