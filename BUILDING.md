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

## The browser build

```
tools/web.sh serve
```

is the whole of it - there is no target to add by hand. `wasm32-unknown-unknown`
is named in the root `rust-toolchain.toml`, so rustup installs it the first time
a build asks. It is named there rather than left to `rustup target add` because
that command adds the target to whichever toolchain is *active where it is run*,
and outside this directory that is not the pinned one; the wasm build then fails
with `can't find crate for core` while `rustup target list --installed` cheerfully
reports the target as present. `tools/web.sh` writes `dist/web`: the wasm, the page, the
JS plugin, and the sound packs. **The packs have to be served alongside** -
they are fetched at runtime rather than baked into the binary, so a deploy that
copies only the wasm plays silently.

It carries the phone layout: a letterboxed field with a drawn pad under it,
which is the thing worth testing and the thing an Android build would need
first. Native builds show the same layout with **F2**, so it can be checked with
a mouse.

### Sound and settings, and why they look the way they do

Settings live under one localStorage key, in the same JSON a desktop writes to
disk - `parse` and `encode` in `persist.rs` are shared, so the two stores can
never drift into reading it differently.

Sound is Web Audio, driven from `web/erect_web.js`. Not macroquad's `audio`
feature: that pulls quad-snd, which pulls quad-alsa-sys, which claims the native
`alsa` library that cpal already claims for the desktop mixer. Cargo resolves
one graph for the whole package, so even a wasm-only feature makes them collide
and nothing builds at all.

Two things about a browser shape the rest:

- **It will not decode audio until the page has been touched.** An AudioContext
  starts suspended and a suspended one never finishes decoding, so the game asks
  for a tap *before* it starts loading. A progress bar shown first would sit at
  zero until the player happened to press something.
- **Nine files have to arrive before anything can play**, because the six music
  layers are one piece of music and must start together. Hence the loading
  screen, and hence one call that starts all six at a single scheduled instant
  rather than six calls that each start "now".

One pack a session: the roll happens when the page loads, and reloading is what
changes the music. A desktop holds all three at once and swaps on reaching the
menu; here that would mean fetching another nine megabytes mid-game.

## Android

```
tools/android.sh            # dist/erect.apk
tools/android.sh install    # and push it to a connected device
```

It is the browser build in a window: one activity holding a WebView, the whole
of `dist/web` as assets. No Gradle - the project is one Java file and one
directory, and the SDK's own tools do it in five steps, which means no wrapper
to keep current, no daemon, and nothing fetched at build time.

The APK carries **no permissions at all**. Everything is inside the package and
the game never reaches the network, which is worth keeping true: an offline game
asking for INTERNET is a question the player has no way to answer.

### Why it serves itself over https

A page loaded from `file:///android_asset/` gets an *opaque* origin. An opaque
origin has no localStorage to save settings into, and `fetch` from one is
refused as cross-origin - which would take the soundtrack with it. So the
activity answers every request for `https://appassets.androidplatform.net/` out
of the assets instead. The host is reserved by Android for exactly this and is
never resolved; the page gets a real, stable, secure origin, and saves and sound
behave as they do in a browser.

Two content types have to be right and are set explicitly: `application/wasm`,
because the module is instantiated with `WebAssembly.compileStreaming` which
refuses anything else, and `audio/flac`, because a browser decides what a file
is from the header and not from its name.

The debug key is generated once into `android/debug.keystore` and kept. A key
regenerated per build would make every install look like a different app to the
device, which means losing the player's saved settings on every rebuild.

**Prerequisite**: a JDK and an Android SDK with build-tools and a platform.

```
sdkmanager 'build-tools;34.0.0' 'platforms;android-34'
```

`ANDROID_HOME` is honoured; without it the script looks where Homebrew's
`android-commandlinetools` puts them. Nothing else - no Android Studio, no
Gradle, no `platform-tools` unless you want `install`, which needs `adb`.

## Releases are built by GitHub

`.github/workflows/release.yml` builds every artefact and attaches them to a
release. It runs on a tag beginning with `v`, and on a button in the Actions tab
for a dry run that builds everything and publishes nothing.

```
git tag v0.1.0
git push origin v0.1.0
```

Each target is built on a runner that *is* that platform, rather than in the
container `tools/release.sh` reaches for on a workstation - a runner is already
a pinned image, so the reproducibility the container buys is already paid for.
`ERECT_NATIVE=1` is what tells the script so.

The tests run first and everything else depends on them, so a red suite
publishes nothing.

### What has to be set up in GitHub

Almost nothing. In order:

1. **Push the repository.** Actions are on by default.
2. **Nothing to configure for permissions.** The workflow asks for
   `contents: write` itself, which is enough to create a release. If the
   organisation has set default workflow permissions to read-only *and*
   forbidden workflows from raising them, then Settings → Actions → General →
   Workflow permissions has to allow read and write.
3. **`ubuntu-24.04-arm` is free on public repositories.** On a private one the
   arm64 Linux runner is billed; drop that line from the matrix if that matters,
   and the aarch64 archive with it.

### Signing the APK for real

Without secrets the APK is signed with a debug key. It installs and it plays -
but it can never update one signed with a different key, so a release that
switches keys asks every player to uninstall first. Set these four secrets in
Settings → Secrets and variables → Actions before the first published release,
and then leave them alone:

| secret | what it is |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | the keystore file, base64 |
| `ANDROID_KEYSTORE_PASSWORD` | its store password |
| `ANDROID_KEY_PASSWORD` | the key password, if different |
| `ANDROID_KEY_ALIAS` | the alias inside it |

To make one:

```
keytool -genkeypair -v -keystore erect-release.keystore \
    -alias erect -keyalg RSA -keysize 2048 -validity 10000
base64 -i erect-release.keystore | pbcopy      # -w0 on Linux
```

Keep the file. Losing it means never being able to update the app for anyone
who installed it.

## Prerequisites

**All targets**

- [rustup](https://rustup.rs), and nothing else. Toolchains, targets and
  components install themselves: the repo pins them in `rust-toolchain.toml` at
  the root and in `erect-psp/`, and rustup fetches whatever a build asks for the
  first time it is needed. The root file names the wasm target; the PSP one
  names its nightly and `rust-src`.

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

Runs on the host and covers `erect-core` and `erect-audio` — 272 tests. The
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
