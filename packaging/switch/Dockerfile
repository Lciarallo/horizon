# Build environment for the Nintendo Switch homebrew port (docs/switch.md).
# devkitA64 + libnx + switch-mesa come from devkitPro's image; Rust nightly adds
# rust-src for -Z build-std on the custom aarch64-nintendo-switch-newlib target.
FROM docker.io/devkitpro/devkita64:latest

ENV DEVKITPRO=/opt/devkitpro \
    DEVKITA64=/opt/devkitpro/devkitA64 \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/opt/devkitpro/devkitA64/bin:/opt/devkitpro/tools/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

ARG RUST_NIGHTLY=nightly-2026-11-10
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates build-essential pkg-config python3 clang \
 && rm -rf /var/lib/apt/lists/* \
 && curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain ${RUST_NIGHTLY} \
 && rustup component add rust-src --toolchain ${RUST_NIGHTLY} \
 && chmod -R a+rwX /opt/rustup /opt/cargo

# std counts the Switch's cores (patch-std.py).
COPY patch-std.py /opt/patch-std.py
RUN python3 /opt/patch-std.py

# std's own libc dependency must be the patched one (third_party/libc: the
# Switch's newlib layouts). The repository is mounted at /work by switch/build.sh.
RUN S=$(rustc --print sysroot)/lib/rustlib/src/rust/library \
 && printf '\n# Nintendo Switch port: newlib/libnx layouts (third_party/libc)\nlibc = { path = "/work/third_party/libc" }\n' >> $S/Cargo.toml \
 && chmod -R a+rwX $S
