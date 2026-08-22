#!/usr/bin/env bash
#
# Builds shippable archives for every platform E-Rect targets.
#
#   tools/release.sh                 all of them
#   tools/release.sh psp macos       just those
#
# Targets: macos windows linux-x86_64 linux-aarch64 psp
#
# macOS and PSP are built natively on the host. The other three go through a
# Debian container, which is what makes them reproducible: the Linux binaries
# then depend on Debian 12's glibc rather than on whatever the host happens to
# have, and the Windows one is linked by a pinned mingw.
#
# ERECT_NATIVE=1 skips the container and builds with the host's own cargo. That
# is for CI, where the runner *is* the target machine and already pins its own
# image - a container inside it would only add a layer and a download. It is a
# poor idea on a workstation, where what the binary ends up requiring is
# whatever happened to be installed.
#
# Everything lands in dist/ as a directory plus a zip of it.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DIST="$ROOT/dist"
IMAGE=erect-build
# Container builds keep their own target directory. Sharing one with the host
# would have two operating systems writing the same fingerprints and force a
# full rebuild on every switch.
DOCKER_TARGET=target-docker

ALL_TARGETS=(macos windows linux-x86_64 linux-aarch64 psp)

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

# --- shared packaging -------------------------------------------------------

# The pack directories are read off disk rather than listed here, so a new pack
# ships as soon as it exists. They have to match erect_audio::packs::PACKS.
pack_dirs() {
    find "$ROOT/packs" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort
}

# The font is embedded in every build - in the binary on a desktop, in the
# baked atlas on the PSP - and the OFL wants its notice to travel with it, so
# both licences ship in every archive.
stage_licences() {
    cp "$ROOT/LICENSE" "$1/LICENSE"
    cp "$ROOT/LICENSE-Tiny5.txt" "$1/LICENSE-Tiny5.txt"
}

# stage_desktop <stage-dir> <built-binary> <readme> <binary-name>
stage_desktop() {
    local stage=$1 binary=$2 readme=$3 name=$4
    rm -rf "$stage"
    mkdir -p "$stage"
    cp "$binary" "$stage/$name"
    cp "$readme" "$stage/README.txt"
    stage_licences "$stage"
    # Only the desktop encoding ships: the 8-bit PSP copies would double the
    # download for files this build can never read.
    local p
    for p in $(pack_dirs); do
        mkdir -p "$stage/packs/$p"
        cp -R "$ROOT/packs/$p/desktop" "$stage/packs/$p/desktop"
    done
}

# zip_stage <stage-dir> <zip-path>
#
# `zip` is not on a GitHub Windows runner, and 7-Zip is not on much else, so
# whichever is there does the job. Python is the last resort and is on every
# runner and every developer machine this repository already needs one on.
zip_stage() {
    local stage=$1 out=$2
    rm -f "$out"
    if command -v zip >/dev/null 2>&1; then
        ( cd "$stage" && zip -q -r -X "$out" . )
    elif command -v 7z >/dev/null 2>&1; then
        ( cd "$stage" && 7z a -tzip -bso0 -bsp0 "$out" . >/dev/null )
    else
        ( cd "$stage" && python3 -c '
import os, sys, zipfile
out = sys.argv[1]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for root, _, files in os.walk("."):
        for f in sorted(files):
            path = os.path.join(root, f)
            z.write(path, os.path.relpath(path, "."))
' "$out" )
    fi
    printf '  %s (%s)\n' "$(basename "$out")" "$(du -h "$out" | cut -f1)"
}

# --- container ---------------------------------------------------------------

# An image holds one architecture, so each platform gets its own tag. Sharing
# a single tag would silently hand an arm64 image to an amd64 build.
image_tag() {
    case "$1" in
        "")            echo "$IMAGE:host" ;;
        linux/amd64)   echo "$IMAGE:amd64" ;;
        linux/arm64)   echo "$IMAGE:arm64" ;;
        *)             echo "$IMAGE:$(echo "$1" | tr '/' '-')" ;;
    esac
}

ensure_image() {
    local platform=$1 tag
    tag=$(image_tag "$platform")
    docker info >/dev/null 2>&1 || die "the Docker daemon is not running"
    if ! docker image inspect "$tag" >/dev/null 2>&1; then
        say "building $tag"
        local plat_flag=""
        [ -n "$platform" ] && plat_flag="--platform $platform"
        docker build -q $plat_flag -t "$tag" "$ROOT/tools/docker" >/dev/null
    fi
}

# docker_build <docker-platform> <rust-target> <tag>
#
# Runs as root inside, then hands the output back to the invoking user - Docker
# on Linux would otherwise leave a root-owned target directory behind.
docker_build() {
    local platform=$1 target=$2 tag=$3
    ensure_image "$platform"
    # Left empty for a cross-compile that does not care about the container's
    # own architecture. Deliberately unquoted so an empty value expands to
    # nothing at all rather than to an empty argument.
    local plat_flag=""
    [ -n "$platform" ] && plat_flag="--platform $platform"
    docker run --rm $plat_flag \
        -v "$ROOT:/work" \
        -e CARGO_TARGET_DIR="/work/$DOCKER_TARGET/$tag" \
        "$(image_tag "$platform")" \
        bash -c "
            set -e
            cargo build --release -p erect-desktop --target $target
            chown -R $(id -u):$(id -g) /work/$DOCKER_TARGET
        "
}

# --- targets -----------------------------------------------------------------

build_macos() {
    say "macOS (host)"
    [ "$(uname -s)" = Darwin ] || die "macos target has to be built on a Mac"
    ( cd "$ROOT" && cargo build --release -p erect-desktop )
    local stage="$DIST/erect-macos"
    stage_desktop "$stage" "$ROOT/target/release/erect" \
        "$ROOT/tools/dist-readme/macos.txt" erect
    zip_stage "$stage" "$DIST/erect-macos.zip"
}

# Builds for <rust-target> on the host, and answers where the binary landed.
host_build() {
    local target=$1
    ( cd "$ROOT" && cargo build --release -p erect-desktop --target "$target" )
    printf '%s' "$ROOT/target/$target/release"
}

build_linux() {
    local arch=$1 platform target
    case "$arch" in
        x86_64)  platform=linux/amd64; target=x86_64-unknown-linux-gnu ;;
        aarch64) platform=linux/arm64; target=aarch64-unknown-linux-gnu ;;
        *) die "unknown Linux arch: $arch" ;;
    esac
    local built
    if [ "${ERECT_NATIVE:-}" = 1 ]; then
        say "Linux $arch (host)"
        built=$(host_build "$target")
    else
        say "Linux $arch (container)"
        docker_build "$platform" "$target" "linux-$arch"
        built="$ROOT/$DOCKER_TARGET/linux-$arch/$target/release"
    fi
    local stage="$DIST/erect-linux-$arch"
    stage_desktop "$stage" "$built/erect" \
        "$ROOT/tools/dist-readme/linux-$arch.txt" erect
    zip_stage "$stage" "$DIST/erect-linux-$arch.zip"
}

build_windows() {
    local built
    if [ "${ERECT_NATIVE:-}" = 1 ]; then
        # A Windows runner builds with MSVC, not mingw. Same binary to a player;
        # the only reason the container uses mingw is that it is not Windows.
        say "Windows x86_64 (host)"
        built=$(host_build x86_64-pc-windows-msvc)
    else
        say "Windows x86_64 (container, mingw)"
        # Built on whatever the host's native platform is: this is a
        # cross-compile either way, so there is nothing to gain from emulating
        # x86 to do it.
        docker_build "" x86_64-pc-windows-gnu windows
        built="$ROOT/$DOCKER_TARGET/windows/x86_64-pc-windows-gnu/release"
    fi
    local stage="$DIST/erect-windows"
    stage_desktop "$stage" "$built/erect.exe" \
        "$ROOT/tools/dist-readme/windows.txt" erect.exe
    zip_stage "$stage" "$DIST/erect-windows.zip"
}

build_psp() {
    say "PSP (host, cargo-psp)"
    command -v cargo-psp >/dev/null 2>&1 || die "cargo-psp is missing: cargo install cargo-psp"
    ( cd "$ROOT/erect-psp" && cargo psp --release )

    local stage="$DIST/erect-psp"
    local game="$stage/PSP/GAME/ERECT"
    rm -rf "$stage"
    mkdir -p "$game/pack"
    cp "$ROOT/erect-psp/target/mipsel-sony-psp/release/erect-psp.EBOOT.PBP" \
        "$game/EBOOT.PBP"
    cp "$ROOT/tools/dist-readme/psp.txt" "$stage/README.txt"
    stage_licences "$stage"
    local p
    for p in $(pack_dirs); do
        cp -R "$ROOT/packs/$p/psp" "$game/pack/$p"
    done
    zip_stage "$stage" "$DIST/erect-psp.zip"
}

# --- entry point --------------------------------------------------------------

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=("${ALL_TARGETS[@]}")

mkdir -p "$DIST"
for t in "${targets[@]}"; do
    case "$t" in
        macos)          build_macos ;;
        windows)        build_windows ;;
        linux-x86_64)   build_linux x86_64 ;;
        linux-aarch64)  build_linux aarch64 ;;
        psp)            build_psp ;;
        *) die "unknown target: $t (known: ${ALL_TARGETS[*]})" ;;
    esac
done

say "done"
ls -1 "$DIST"/*.zip 2>/dev/null || true
