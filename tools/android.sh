#!/usr/bin/env bash
#
# Packs the browser build into an APK.
#
#   tools/android.sh            build dist/erect.apk
#   tools/android.sh install    build, then push it to a connected device
#
# No Gradle. The project is one activity and one asset directory, and the SDK's
# own tools do the whole job in five steps - which means no wrapper to keep
# current, no daemon, and nothing fetched from the network at build time.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT="$ROOT/dist"
WORK="$OUT/android-build"
APK="$OUT/erect.apk"

# Where the command-line tools put things. Overridable, because a machine with
# Android Studio keeps them somewhere else entirely.
SDK=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/homebrew/share/android-commandlinetools}}
BUILD_TOOLS=${BUILD_TOOLS:-$(ls -d "$SDK"/build-tools/* 2>/dev/null | sort -V | tail -1)}
PLATFORM=${PLATFORM:-$(ls -d "$SDK"/platforms/* 2>/dev/null | sort -V | tail -1)}

if [ ! -d "${BUILD_TOOLS:-}" ] || [ ! -f "${PLATFORM:-}/android.jar" ]; then
    echo "no Android SDK under $SDK" >&2
    echo "install one with:  sdkmanager 'build-tools;34.0.0' 'platforms;android-34'" >&2
    exit 1
fi

echo "== web build"
"$ROOT/tools/web.sh" >/dev/null

rm -rf "$WORK"
mkdir -p "$WORK/assets" "$WORK/classes" "$WORK/dex"
cp -R "$OUT/web/." "$WORK/assets/"

echo "== compiling"
javac --release 11 -nowarn \
    -classpath "$PLATFORM/android.jar" \
    -d "$WORK/classes" \
    $(find "$ROOT/android/app/src" -name '*.java')

"$BUILD_TOOLS/d8" --release --lib "$PLATFORM/android.jar" \
    --output "$WORK/dex" \
    $(find "$WORK/classes" -name '*.class')

echo "== packaging"
"$BUILD_TOOLS/aapt2" compile --dir "$ROOT/android/res" -o "$WORK/res.zip"
"$BUILD_TOOLS/aapt2" link \
    -I "$PLATFORM/android.jar" \
    --manifest "$ROOT/android/AndroidManifest.xml" \
    --min-sdk-version 24 \
    --target-sdk-version 34 \
    -A "$WORK/assets" \
    -o "$WORK/unsigned.apk" \
    "$WORK/res.zip"

# The dex has to sit at the root of the archive, which is why this is a `cd`
# rather than a path on the command line.
( cd "$WORK/dex" && zip -q "$WORK/unsigned.apk" classes.dex )

echo "== signing"
# Anything installable has to be signed, and *what it is signed with* decides
# whether an update is an update. A device treats a differently-signed package
# as a different app: it will not install over the old one, and the player's
# saved settings go with it. So the key is either the release key handed in by
# the environment, or one debug key made once and kept - never a fresh one per
# build.
if [ -n "${ANDROID_KEYSTORE_BASE64:-}" ]; then
    echo "  release key, from the environment"
    KEYSTORE="$WORK/release.keystore"
    # `base64 -d` on GNU, `-D` on BSD; -D is rejected by GNU and vice versa.
    printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 -d > "$KEYSTORE" 2>/dev/null \
        || printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 -D > "$KEYSTORE"
    STORE_PASS=${ANDROID_KEYSTORE_PASSWORD:?ANDROID_KEYSTORE_PASSWORD is not set}
    KEY_PASS=${ANDROID_KEY_PASSWORD:-$STORE_PASS}
    KEY_ALIAS=${ANDROID_KEY_ALIAS:?ANDROID_KEY_ALIAS is not set}
else
    KEYSTORE="$ROOT/android/debug.keystore"
    STORE_PASS=android
    KEY_PASS=android
    KEY_ALIAS=erect
    if [ ! -f "$KEYSTORE" ]; then
        keytool -genkeypair -v \
            -keystore "$KEYSTORE" -storepass "$STORE_PASS" -keypass "$KEY_PASS" \
            -alias "$KEY_ALIAS" -keyalg RSA -keysize 2048 -validity 10000 \
            -dname "CN=E-Rect debug, OU=, O=, L=, S=, C=" >/dev/null 2>&1
        echo "  made a debug key at android/debug.keystore"
    fi
fi

"$BUILD_TOOLS/zipalign" -f -p 4 "$WORK/unsigned.apk" "$APK"
"$BUILD_TOOLS/apksigner" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$STORE_PASS" --key-pass "pass:$KEY_PASS" \
    --ks-key-alias "$KEY_ALIAS" "$APK"
# The decoded key is a secret sitting in the build directory; the build
# directory is not.
[ -n "${ANDROID_KEYSTORE_BASE64:-}" ] && rm -f "$KEYSTORE"

printf '== done: %s (%s)\n' "$APK" "$(du -h "$APK" | cut -f1)"

if [ "${1:-}" = "install" ]; then
    echo "== installing"
    adb install -r "$APK"
fi
