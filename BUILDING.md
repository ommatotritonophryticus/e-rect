# Building E-Rect

Five artefacts ship: macOS, Windows, Linux x86_64, Linux aarch64 and PSP.
One script builds and packages any of them.

```
tools/release.sh                 # everything
tools/release.sh psp macos       # just these
```

The browser build is separate, because it is a different kind of thing - a page
rather than an archive:

```
tools/web.sh                     # build into dist/web
tools/web.sh serve               # build, then host it on every interface
```

`serve` binds to every address and prints them, because the reason this target
exists is to open the game on a phone - and a phone cannot reach another
machine's localhost.

Each target lands in `dist/` as both a directory and a zip of it, laid out the
way it is meant to be installed.

## What builds where, and why

| Target | Built by | Where |
| --- | --- | --- |
| macOS (Apple Silicon) | host `cargo` | native |
| PSP | host `cargo psp` | native |
| Linux x86_64 | Debian 12 container | `tools/docker/Dockerfile` |
| Linux aarch64 | Debian 12 container | same |
| Windows x86_64 | same container, mingw | same |
| Browser (wasm) | `tools/web.sh` | `web/` |

The three cross targets go through a container on purpose. A Linux binary
built there links against Debian 12's glibc, which is a floor you can state in
a README; one built against whatever the host has is a binary whose
requirements nobody knows. The Windows build needs mingw, and the container is
where a pinned mingw lives.

## The browser build is deliberately silent

It carries no sound and forgets its settings when the tab closes. Both are the
same missing thing: a browser has no filesystem to read packs from and no audio
device to hand the mixer, and the game has always treated sound as optional -
`Sound::start` returning an error is a supported outcome, not a failure.

What it does carry is the phone layout: a letterboxed field with a drawn pad
under it, which is the thing worth testing and the thing an Android build would
need first. Native builds show the same layout with **F2**, so it can be checked
with a mouse.

```
rustup target add wasm32-unknown-unknown
```

is the only extra prerequisite.

## Prerequisites

**All targets**

- [rustup](https://rustup.rs). Toolchains install themselves: the repo pins
  them in `rust-toolchain.toml` at the root and in `erect-psp/`, and rustup
  fetches whatever a build asks for the first time it is needed.

**PSP**

```
cargo install cargo-psp
```

Verified with cargo-psp 0.2.9 against the `psp` crate 0.3.13. The nightly it
needs (`nightly-2026-08-02`, with `rust-src`) comes from
`erect-psp/rust-toolchain.toml`, so there is nothing to select by hand — and no
`rustup override` to set, which is the setup that does *not* survive a clone.

**Windows and Linux**

Docker, with the daemon running. The image builds itself on first use and is
cached after that. On Apple Silicon the x86_64 Linux build runs under
emulation, so expect it to take several minutes; the other two are native.

**Building on Linux for Linux, without a container**

If you would rather build directly:

```
sudo apt install pkg-config libasound2-dev libudev-dev   # Debian/Ubuntu
cargo build --release -p erect-desktop
```

`cpal` needs the ALSA headers and `gilrs` needs libudev. X11 and OpenGL are
dlopened at runtime by macroquad, so they are not needed to compile.

**Rebuilding sound packs** (only if you change the source audio)

python3 with numpy, and ffmpeg on the path. Verified with Python 3.13 and
ffmpeg 8.1. The generated packs are committed, so this is not part of a normal
build.

## Toolchains are pinned to exact releases

`rust-toolchain.toml` names `1.97.1` at the root and `nightly-2026-08-02` in
`erect-psp/`. Both are the compilers each target was last verified against.

The nightly date is exact for a reason. An open `nightly` drifts onto compilers
that `cargo-psp` and the `psp` crate have never seen, and that breakage lands
on whoever builds next rather than on whoever moved the pin. Moving it is a
deliberate edit, and the build that follows is the test.

Newer stables are expected to work for the desktop targets; raise the number
when you move.

## Running the tests

```
cargo test
```

Runs on the host and covers `erect-core` and `erect-audio` — 253 tests. The
frontends are mostly not covered; drawing is the part a test cannot see.

`erect-psp` is excluded from the workspace, because it only builds for
`mipsel-sony-psp` and its presence would break a plain `cargo build`. Build it
from inside its own directory.

### Checking what actually reaches the screen

A marker the renderer never draws looks exactly like a working one from the
test suite. The desktop frontend can run itself and save what it drew:

```
cargo run --release -p erect-desktop --features harness
```

with `ERECT_HARNESS` set, comma separated:

| key | meaning |
| --- | --- |
| `wave=N` | start there, through the developer parameters |
| `score=N` | start with that score |
| `frames=N` | how long to run before stopping |
| `out=PATH` | save a PNG at the end |
| `at=N:PATH` | save one at frame `N` |
| `on_elite=PATH` | save the frame a rolled heavy is announced |
| `alive` | keep the players standing |
| `boons` | hand the players every standing upgrade |
| `screen=dev\|title` | open that menu instead of playing, to look at a layout |
| `wall=pull\|push` | give them a modified wall and raise one every 90 ticks |
| `fight` | swing and follow the nearest enemy |

`alive` and `fight` are usually both wanted: nothing drives the player
otherwise, and a wave only spawns as fast as the field is cleared, so a run
that kills nothing stalls at the crowd limit and never reaches its later half.

```
ERECT_HARNESS="wave=7,alive,fight,frames=2000,on_elite=/tmp/heavy.png" \
  cargo run --release -p erect-desktop --features harness
```

The feature is off by default and compiled out of every shipped build.

## Known noise

The PSP link step prints `relocation refers to a discarded section` warnings
out of `compiler_builtins` (its `libm` maths) and the `psp` crate's VFPU
context. The build completes and the EBOOT is the expected size. They are
warnings from nightly's `linker_messages` lint, not errors.
