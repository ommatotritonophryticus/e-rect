// Browser-side services the game asks for by name.
//
// Registered as a miniquad plugin: `register_plugin` is handed the wasm import
// object before instantiation, and anything put on `env` becomes callable from
// Rust as a plain `extern "C"` function. `wasm_memory` and `UTF8ToString` come
// from mq_js_bundle.js, which is why this file must load after it.
"use strict";

(function () {
    const KEY = "erect.settings";

    // Every one of these can throw rather than return an error: a private
    // window refuses localStorage outright, and a full quota throws on write.
    // None of that is worth interrupting a game over, so each one fails quiet
    // and the caller sees "nothing stored", which is what a first run is.
    function write(ptr, len) {
        try {
            const bytes = new Uint8Array(wasm_memory.buffer, ptr, len);
            localStorage.setItem(KEY, new TextDecoder().decode(bytes));
        } catch (e) {
            console.warn("erect: settings not saved", e);
        }
    }

    function read(ptr, cap) {
        try {
            const text = localStorage.getItem(KEY);
            if (text === null) {
                return 0;
            }
            const bytes = new TextEncoder().encode(text);
            // Refusing to write past the buffer is the caller's whole safety
            // guarantee here; a truncated document would parse as garbage.
            if (bytes.length > cap) {
                return 0;
            }
            new Uint8Array(wasm_memory.buffer, ptr, bytes.length).set(bytes);
            return bytes.length;
        } catch (e) {
            console.warn("erect: settings not read", e);
            return 0;
        }
    }

    miniquad_add_plugin({
        register_plugin: function (importObject) {
            importObject.env.erect_storage_write = write;
            importObject.env.erect_storage_read = read;
        },
        version: 1,
        name: "erect_web",
    });
})();

/* ---------------- audio ---------------- */

// Web Audio directly, rather than through macroquad's `audio` feature.
//
// That feature pulls quad-snd, which pulls quad-alsa-sys, which claims the
// native `alsa` library - and cpal, the desktop mixer's output, claims it too.
// Cargo resolves one dependency graph for the whole package, so even a
// wasm-only feature makes the two collide and nothing builds. Going straight to
// the browser's own API avoids the crate entirely, and buys something as well:
// macroquad starts sounds with `start(0)`, whereas the six music layers want a
// single shared instant, and that is the one thing this game's music cannot do
// without.
(function () {
    const AC = window.AudioContext || window.webkitAudioContext;
    let ctx = null;
    // Decoded buffers by id. Ids start at 1 so 0 can mean "no such sound".
    const buffers = new Map();
    // Ids that failed to decode, kept apart so a caller can tell "not yet" from
    // "never".
    const broken = new Set();
    let nextId = 1;
    // One gain node per music layer, and the source feeding it. Layers are held
    // for the life of the page: they loop forever and are only ever silenced.
    const layers = [];
    let sfxGain = null;

    function context() {
        if (ctx === null) {
            ctx = new AC();
        }
        return ctx;
    }

    // Decoding will not finish while the context is suspended, which is where a
    // browser leaves it until the page has been interacted with. The game waits
    // for a tap before it asks for any of this, so by here the resume is a
    // formality - but a formality worth doing, because a page restored from the
    // back/forward cache arrives suspended again.
    function decode(ptr, len) {
        const c = context();
        c.resume();
        const bytes = wasm_memory.buffer.slice(ptr, ptr + len);
        const id = nextId++;
        c.decodeAudioData(
            bytes,
            function (buf) { buffers.set(id, buf); },
            function (e) {
                console.warn("erect: could not decode audio", e);
                broken.add(id);
            }
        );
        return id;
    }

    // 0 still working, 1 ready, 2 gave up.
    function ready(id) {
        if (buffers.has(id)) { return 1; }
        return broken.has(id) ? 2 : 0;
    }

    // Starts every music layer at one instant.
    //
    // A shared start time is the whole reason this is one call taking an array
    // rather than six calls taking one id each: six `start(0)` calls drift by
    // however long the loop between them took, and six layers of one piece of
    // music drifting apart is the one failure this arrangement exists to
    // prevent. Scheduled a moment ahead so the browser has time to line them up
    // rather than starting each as it is reached.
    function musicStart(ptr, count) {
        const c = context();
        c.resume();
        const ids = new Uint32Array(wasm_memory.buffer, ptr, count);
        const at = c.currentTime + 0.12;
        for (let i = 0; i < count; i += 1) {
            const buf = buffers.get(ids[i]);
            if (buf === undefined) { continue; }
            const gain = c.createGain();
            gain.gain.value = 0;
            gain.connect(c.destination);
            const src = c.createBufferSource();
            src.buffer = buf;
            src.loop = true;
            src.connect(gain);
            src.start(at);
            layers[i] = gain;
        }
    }

    // Ramped rather than assigned: the core fades layers over hundreds of
    // milliseconds and hands a new value each frame, and a bare assignment at
    // 60 Hz is a step every 16 ms, which clicks.
    function musicGain(slot, value) {
        const g = layers[slot];
        if (g === undefined) { return; }
        const c = context();
        g.gain.setTargetAtTime(value, c.currentTime, 0.01);
    }

    function sfxPlay(id, volume) {
        const buf = buffers.get(id);
        if (buf === undefined) { return; }
        const c = context();
        if (sfxGain === null) {
            sfxGain = c.createGain();
            sfxGain.connect(c.destination);
        }
        sfxGain.gain.value = volume;
        const src = c.createBufferSource();
        src.buffer = buf;
        src.connect(sfxGain);
        src.start(0);
    }

    miniquad_add_plugin({
        register_plugin: function (importObject) {
            importObject.env.erect_audio_decode = decode;
            importObject.env.erect_audio_ready = ready;
            importObject.env.erect_audio_music_start = musicStart;
            importObject.env.erect_audio_music_gain = musicGain;
            importObject.env.erect_audio_sfx_play = sfxPlay;
        },
        version: 1,
        name: "erect_audio",
    });
})();
