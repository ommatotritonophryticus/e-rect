# Build environment for every target that is not built natively on the host:
# Linux x86_64, Linux aarch64 and Windows x86_64.
#
# The Rust version is pinned to the same release as rust-toolchain.toml, so the
# container does not download a second toolchain when it reads that file.
ARG RUST_VERSION=1.97.1
FROM rust:${RUST_VERSION}-slim-bookworm

# cpal needs the ALSA headers and gilrs needs libudev; both are link-time
# dependencies of a Linux build. macroquad dlopens X11 and OpenGL at runtime
# and so needs nothing here - which is why a container with no GUI stack can
# still produce a working binary.
#
# The mingw cross-compiler is what links the Windows target. Debian ships it
# for arm64 hosts too, so an Apple Silicon machine builds erect.exe without
# emulating x86.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libasound2-dev \
        libudev-dev \
        gcc-mingw-w64-x86-64 \
        zip \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu

WORKDIR /work
