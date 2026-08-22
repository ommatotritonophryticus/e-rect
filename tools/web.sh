#!/usr/bin/env bash
#
# Builds the browser version into dist/web and, with `serve`, hosts it.
#
#   tools/web.sh          build only
#   tools/web.sh serve    build, then serve on every interface
#
# Serving on 0.0.0.0 is the point: the reason this build exists is to open it on
# a phone, and a phone cannot reach localhost on another machine.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT="$ROOT/dist/web"
PORT=${PORT:-8080}

echo "== building for the browser"
( cd "$ROOT" && cargo build --release -p erect-desktop --target wasm32-unknown-unknown )

rm -rf "$OUT"
mkdir -p "$OUT"
cp "$ROOT/target/wasm32-unknown-unknown/release/erect.wasm" "$OUT/"
cp "$ROOT/web/index.html" "$ROOT/web/mq_js_bundle.js" "$ROOT/web/erect_web.js" "$OUT/"

# The soundtrack is fetched at runtime rather than baked into the wasm, so the
# packs have to sit beside the page. Only the desktop encoding: the PSP's 8-bit
# wavs are half the size and sound it, and a browser decodes FLAC natively.
for pack in "$ROOT"/packs/*/; do
    name=$(basename "$pack")
    mkdir -p "$OUT/packs/$name"
    cp -R "$pack/desktop" "$OUT/packs/$name/"
done

printf '  %s (%s)\n' erect.wasm "$(du -h "$OUT/erect.wasm" | cut -f1)"
printf '  %s (%s)\n' packs "$(du -sh "$OUT/packs" | cut -f1)"

if [ "${1:-}" != "serve" ]; then
    echo "== done: $OUT"
    exit 0
fi

# Every address the phone might use, so the URL can just be read off.
echo
echo "== open one of these on the phone, on the same network:"
for ip in $(ipconfig getifaddr en0 2>/dev/null; ipconfig getifaddr en1 2>/dev/null; hostname -I 2>/dev/null); do
    echo "     http://$ip:$PORT"
done
echo "     http://localhost:$PORT"
echo
python3 -m http.server "$PORT" --directory "$OUT"
