#!/usr/bin/env python3
"""Builds a runtime sound pack from a source pack.

A source pack is a directory of audio files plus a `source.json` describing what
each file is for. This produces a runtime pack holding both encodings the game
needs:

    desktop/  FLAC, stereo, lossless - exact sample counts, so loops stay tight
    psp/      8-bit PCM, mono, peak-normalised per layer

Why not ADPCM for the PSP, when it is half the size? Because this music is
chiptune. IMA ADPCM is a slew-limited predictive codec, and a square wave is the
worst thing you can feed it: measured on a 440 Hz square it returns 5.8 dB SNR
against 47.1 dB for plain 8-bit. On the real layers 8-bit wins by 10-23 dB. The
edges of the waveform *are* the sound here, so they are what has to survive.

Each PSP layer is normalised to full scale before quantising - 8-bit has a fixed
noise floor, so a quiet layer would otherwise throw away most of its resolution.
The gain needed to put it back at the intended level is recorded in the manifest
and applied by the mixer, which keeps the balance identical to the desktop.

Music layers are trimmed to one identical sample range so a single shared
playhead keeps them in sync by construction.

Usage:
    python3 build_pack.py --src ../audio/pack1 --out ../packs/pack1
    python3 build_pack.py --src ../audio/pack1 --out ../packs/pack1 --psp-format pcm_s16le
"""

import argparse
import json
import math
import pathlib
import shutil
import subprocess
import sys
import wave

import numpy as np

SR = 44100


def run(*args):
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"command failed: {' '.join(args)}\n{r.stderr.strip()}")
    return r


def decode(path, tmp):
    """Decodes anything ffmpeg reads into float32 samples, shape (n, channels)."""
    out = tmp / (pathlib.Path(path).stem + ".dec.wav")
    run("ffmpeg", "-v", "error", "-y", "-i", str(path), "-c:a", "pcm_s16le", str(out))
    with wave.open(str(out), "rb") as w:
        if w.getframerate() != SR:
            sys.exit(f"{path}: expected {SR} Hz, got {w.getframerate()}")
        data = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
        data = data.reshape(-1, w.getnchannels())
    return data.astype(np.float32) / 32768.0


def write_wav(path, samples, channels):
    clipped = np.clip(samples, -1.0, 1.0)
    pcm = (clipped * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def db(x):
    return 20.0 * math.log10(max(float(x), 1e-12))


def encode_desktop(samples, channels, out_path, tmp):
    stem = tmp / (out_path.stem + ".d.wav")
    write_wav(stem, samples, channels)
    run("ffmpeg", "-v", "error", "-y", "-i", str(stem),
        "-c:a", "flac", "-compression_level", "8", str(out_path))


def encode_psp(samples, out_path, tmp, fmt):
    """Mono, peak-normalised. Returns the dB the mixer must re-apply.

    Matching RMS rather than peak keeps the balance between layers the same as
    on the desktop: downmixing decorrelated stereo to mono costs about 3 dB, and
    it costs it only on the layers that are actually stereo.
    """
    mono = samples.mean(axis=1) if samples.ndim > 1 else samples
    target_rms = float(np.sqrt((samples ** 2).mean()))

    peak = float(np.abs(mono).max())
    if peak <= 0:
        norm = mono
        makeup = 0.0
    else:
        norm = mono / peak * 0.98
        norm_rms = float(np.sqrt((norm ** 2).mean()))
        makeup = db(target_rms) - db(norm_rms)

    stem = tmp / (out_path.stem + ".p.wav")
    write_wav(stem, norm, 1)
    run("ffmpeg", "-v", "error", "-y", "-i", str(stem), "-ac", "1", "-c:a", fmt,
        str(out_path))
    return makeup, norm


def measure_snr(reference, encoded_path, tmp):
    """SNR of the encoded file against the signal that went in."""
    out = tmp / "snr.wav"
    run("ffmpeg", "-v", "error", "-y", "-i", str(encoded_path), "-c:a", "pcm_s16le",
        str(out))
    with wave.open(str(out), "rb") as w:
        got = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
        got = got.reshape(-1, w.getnchannels()).astype(np.float32).mean(axis=1) / 32768.0
    n = min(len(reference), len(got))
    a, b = reference[:n], got[:n]
    k = float((a * b).sum() / max((b * b).sum(), 1e-12))
    err = a - b * k
    return 10 * math.log10(max(float((a ** 2).sum()), 1e-20)
                           / max(float((err ** 2).sum()), 1e-20))


def decoded_length(path):
    r = run("ffprobe", "-v", "error", "-show_entries", "stream=duration_ts",
            "-of", "csv=p=0", str(path))
    return int(r.stdout.strip().split(",")[0])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True, help="source pack directory")
    ap.add_argument("--out", required=True, help="runtime pack directory to write")
    ap.add_argument("--psp-format", default="pcm_u8", choices=["pcm_u8", "pcm_s16le"],
                    help="pcm_u8 is half the size; pcm_s16le buys ~20 dB on peaky layers")
    args = ap.parse_args()

    src = pathlib.Path(args.src).resolve()
    out = pathlib.Path(args.out).resolve()
    cfg = json.loads((src / "source.json").read_text())

    tmp = out / ".tmp"
    for d in (out / "desktop", out / "psp", tmp):
        d.mkdir(parents=True, exist_ok=True)

    music = cfg["music"]
    start = int(music["trim_start"])
    length = int(music["loop_samples"])

    # --- music -------------------------------------------------------------
    print(f"music: {len(music['layers'])} layers, loop {length} samples "
          f"({length / SR:.4f} s, {music['bars']} bars @ {music['bpm']} BPM)")

    raw = {}
    for layer in music["layers"]:
        a = decode(src / layer["src"], tmp)
        if len(a) < start + length:
            sys.exit(f"{layer['src']}: only {len(a)} samples, need {start + length}")
        raw[layer["id"]] = a[start:start + length]

    # The loudest reachable combination decides the master gain. `group` layers
    # are mutually exclusive, so only the loudest of each group counts.
    groups, loudest = {}, np.zeros((length, 2), np.float32)
    for layer in music["layers"]:
        g = layer.get("group")
        if g is None:
            loudest += raw[layer["id"]]
        else:
            groups.setdefault(g, []).append(layer["id"])
    for ids in groups.values():
        pick = max(ids, key=lambda i: float(np.abs(raw[i]).max()))
        loudest += raw[pick]
        print(f"  group '{ids}': loudest member is '{pick}'")

    peak = float(np.abs(loudest).max())
    gain = 10 ** (cfg["music"]["target_peak_dbfs"] / 20) / peak
    print(f"  loudest state peaks at {db(peak):+.1f} dBFS -> master gain "
          f"{db(gain):+.1f} dB (x{gain:.2f})")

    report_layers, psp_gains = [], {}
    print(f"  {'layer':9} {'peak':>7} {'psp make-up':>12} {'psp SNR':>9}")
    for layer in music["layers"]:
        lid = layer["id"]
        a = raw[lid] * gain

        encode_desktop(a, 2, out / "desktop" / f"{lid}.flac", tmp)
        makeup, norm = encode_psp(a, out / "psp" / f"{lid}.wav", tmp, args.psp_format)
        snr = measure_snr(norm, out / "psp" / f"{lid}.wav", tmp)
        psp_gains[lid] = round(makeup, 2)

        entry = {k: v for k, v in layer.items() if k != "src"}
        entry["file"] = lid
        report_layers.append(entry)
        print(f"  {lid:9} {db(np.abs(a).max()):7.1f} {makeup:11.1f} dB {snr:8.1f} dB"
              + ("   <-- LOW" if snr < 20 else ""))

    # --- sfx ---------------------------------------------------------------
    sfx_cfg = cfg["sfx"]
    print("\nsfx:")
    cut = 10 ** (sfx_cfg["trim_tail_below_dbfs"] / 20)
    sfx_raw = {}
    for snd in sfx_cfg["sounds"]:
        a = decode(src / snd["src"], tmp)
        loud = np.abs(a).max(axis=1)
        keep = np.where(loud > cut)[0]
        end = int(keep[-1]) + 1 if len(keep) else len(a)
        end = min(len(a), end + SR // 100)     # 10 ms so the tail is not clipped
        sfx_raw[snd["id"]] = (a[:end], len(a))

    # One shared gain, so the author's balance between the sounds survives.
    sfx_peak = max(float(np.abs(a).max()) for a, _ in sfx_raw.values())
    sfx_gain = 10 ** (sfx_cfg["target_peak_dbfs"] / 20) / sfx_peak
    print(f"  loudest sfx peaks at {db(sfx_peak):+.1f} dBFS -> gain "
          f"{db(sfx_gain):+.1f} dB (x{sfx_gain:.2f}), applied to all")

    report_sfx = []
    for snd in sfx_cfg["sounds"]:
        sid = snd["id"]
        a, was = sfx_raw[sid]
        a = a * sfx_gain
        encode_desktop(a, a.shape[1], out / "desktop" / f"{sid}.flac", tmp)
        makeup, _ = encode_psp(a, out / "psp" / f"{sid}.wav", tmp, args.psp_format)
        psp_gains[sid] = round(makeup, 2)
        entry = {k: v for k, v in snd.items() if k != "src"}
        entry["file"] = sid
        report_sfx.append(entry)
        print(f"  {sid:9} {was:7} -> {len(a):6} samples "
              f"({was / SR:.2f}s -> {len(a) / SR:.2f}s)  peak {db(np.abs(a).max()):+.1f} dBFS")

    # --- manifest ----------------------------------------------------------
    manifest = {
        "schema": 1,
        "name": cfg["name"],
        "description": cfg.get("description", ""),
        "music": {
            "sample_rate": SR,
            "loop_samples": length,
            "bpm": music["bpm"],
            "bars": music["bars"],
            "baked_gain_db": round(db(gain), 2),
            "layers": report_layers,
        },
        "sfx": {
            "baked_gain_db": round(db(sfx_gain), 2),
            "mute_when": sfx_cfg.get("mute_when", []),
            "sounds": report_sfx,
        },
        "encodings": {
            "desktop": {"dir": "desktop", "format": "flac", "channels": 2,
                        "gains_db": {k: 0.0 for k in psp_gains}},
            "psp": {"dir": "psp", "format": args.psp_format, "channels": 1,
                    "//": "each file is peak-normalised; apply gains_db when mixing",
                    "gains_db": psp_gains},
        },
    }
    (out / "pack.json").write_text(json.dumps(manifest, indent=2) + "\n")

    # --- validation --------------------------------------------------------
    print("\nvalidating:")
    ok = True
    for layer in music["layers"]:
        for enc, ext in (("desktop", "flac"), ("psp", "wav")):
            got = decoded_length(out / enc / f"{layer['id']}.{ext}")
            if got != length:
                print(f"  FAIL {enc}/{layer['id']}: {got} samples, expected {length}")
                ok = False
    if ok:
        print(f"  all {len(music['layers'])} layers decode to exactly {length} samples "
              f"in both encodings")

    shutil.rmtree(tmp)
    d_sz = sum(f.stat().st_size for f in (out / "desktop").iterdir())
    p_sz = sum(f.stat().st_size for f in (out / "psp").iterdir())
    print(f"\ndesktop {d_sz / 1e6:.1f} MB   psp {p_sz / 1e6:.1f} MB   -> {out}")
    if not ok:
        sys.exit(1)


if __name__ == "__main__":
    main()
