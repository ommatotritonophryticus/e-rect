//! Software mixer shared by both platforms.
//!
//! Every music layer reads from one shared playhead, so they cannot drift
//! apart: synchronisation is structural rather than something the code has to
//! maintain. Silent layers cost only the loop of the index they already share.

use alloc::vec::Vec;
use erect_core::audio::{AudioEvent, MusicState};

use crate::gains::GainEngine;

/// Sample data as the pack stored it, so neither platform pays to convert.
/// The PSP ships 8-bit mono to fit its memory; the desktop keeps the stereo the
/// composition was written in.
#[derive(Clone, Copy)]
pub enum Samples<'a> {
    U8Mono(&'a [u8]),
    I16Mono(&'a [i16]),
    /// Interleaved left/right.
    I16Stereo(&'a [i16]),
}

impl Samples<'_> {
    /// Frames, not samples: a stereo frame is one position on the playhead.
    #[inline]
    pub fn frames(&self) -> usize {
        match self {
            Samples::U8Mono(s) => s.len(),
            Samples::I16Mono(s) => s.len(),
            Samples::I16Stereo(s) => s.len() / 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    #[inline]
    fn at(&self, frame: usize, ch: usize) -> i32 {
        match self {
            // 8-bit WAV is unsigned with 128 at silence.
            Samples::U8Mono(s) => ((s[frame] as i32) - 128) << 8,
            Samples::I16Mono(s) => s[frame] as i32,
            Samples::I16Stereo(s) => s[frame * 2 + ch] as i32,
        }
    }
}

/// Maps a 0..=100 slider onto a gain.
///
/// Squared rather than linear: loudness is not proportional to amplitude, so a
/// linear slider spends most of its travel in a range that barely changes and
/// then drops off a cliff at the bottom. Squaring puts the halfway point near
/// -12 dB, which reads as "about half as loud".
pub fn volume_to_gain(percent: u32) -> f32 {
    let x = (percent.min(100) as f32) / 100.0;
    x * x
}

pub struct SfxSpec<'a> {
    pub event: AudioEvent,
    pub samples: Samples<'a>,
    pub gain: f32,
    pub max_voices: usize,
    pub min_interval_frames: u64,
}

struct Voice {
    sfx: usize,
    pos: usize,
}

pub struct Mixer<'a> {
    layers: Vec<Samples<'a>>,
    loop_len: usize,
    playhead: usize,
    gains: GainEngine,
    prev_gains: Vec<f32>,

    cur_gains: Vec<f32>,

    sfx: Vec<SfxSpec<'a>>,
    voices: Vec<Voice>,
    last_fired: Vec<u64>,
    frames: u64,

    sample_rate: u32,
    music_gain: f32,
    sfx_gain: f32,
    started: bool,
}

impl<'a> Mixer<'a> {
    pub fn new(
        layers: Vec<Samples<'a>>,
        gains: GainEngine,
        sfx: Vec<SfxSpec<'a>>,
        sample_rate: u32,
    ) -> Self {
        assert_eq!(layers.len(), gains.len(), "one gain per layer");
        let loop_len = layers.iter().map(|s| s.frames()).min().unwrap_or(0);
        let prev_gains = alloc::vec![0.0; layers.len()];
        let cur_gains = alloc::vec![0.0; layers.len()];
        let last_fired = alloc::vec![0u64; sfx.len()];
        Self {
            layers,
            loop_len,
            playhead: 0,
            gains,
            prev_gains,
            cur_gains,
            sfx,
            voices: Vec::new(),
            last_fired,
            frames: 0,
            sample_rate,
            music_gain: 1.0,
            sfx_gain: 1.0,
            started: false,
        }
    }

    /// Player-facing volumes, as the 0..=100 the settings menu shows.
    pub fn set_volumes(&mut self, music_percent: u32, sfx_percent: u32) {
        self.music_gain = volume_to_gain(music_percent);
        self.sfx_gain = volume_to_gain(sfx_percent);
    }

    pub fn loop_len(&self) -> usize {
        self.loop_len
    }

    /// Swaps in a different pack's music without touching the sound effects.
    ///
    /// The playhead restarts and the gains snap rather than fade: this happens
    /// on arriving at the menu or starting a run, where the music is expected to
    /// change outright. Fading between two unrelated compositions would only
    /// sound like a mistake.
    pub fn switch_music(&mut self, layers: Vec<Samples<'a>>, gains: GainEngine) {
        assert_eq!(layers.len(), gains.len(), "one gain per layer");
        self.loop_len = layers.iter().map(|s| s.frames()).min().unwrap_or(0);
        self.prev_gains = alloc::vec![0.0; layers.len()];
        self.cur_gains = alloc::vec![0.0; layers.len()];
        self.layers = layers;
        self.gains = gains;
        self.playhead = 0;
        self.started = false;
    }

    /// Starts a one-shot, honouring the pack's voice cap and retrigger guard.
    pub fn fire(&mut self, event: AudioEvent) {
        let Some(idx) = self.sfx.iter().position(|s| s.event == event) else {
            return;
        };
        let spec = &self.sfx[idx];
        if self.frames < self.last_fired[idx] + spec.min_interval_frames && self.frames > 0 {
            return;
        }
        let live = self.voices.iter().filter(|v| v.sfx == idx).count();
        if live >= spec.max_voices {
            // Steal the oldest of this sound rather than refuse to play.
            if let Some(p) = self.voices.iter().position(|v| v.sfx == idx) {
                self.voices.remove(p);
            }
        }
        self.last_fired[idx] = self.frames;
        self.voices.push(Voice { sfx: idx, pos: 0 });
    }

    /// Renders one block of interleaved stereo.
    ///
    /// Layer gains are recomputed once per block and interpolated across it, so
    /// a 300 ms fade never steps audibly no matter how long the block is.
    pub fn render(&mut self, out: &mut [i16], state: &MusicState, sfx_muted: bool) {
        let frames = out.len() / 2;
        if self.loop_len == 0 || frames == 0 {
            out.fill(0);
            return;
        }

        let dt_ms = frames as f32 * 1000.0 / self.sample_rate as f32;
        if !self.started {
            self.gains.snap(state);
            self.started = true;
        }
        for i in 0..self.layers.len() {
            self.prev_gains[i] = self.gains.gain(i);
        }
        self.gains.update(state, dt_ms);
        for i in 0..self.layers.len() {
            self.cur_gains[i] = self.gains.gain(i);
        }

        let inv = 1.0 / frames as f32;
        for f in 0..frames {
            let t = f as f32 * inv;
            let idx = self.playhead + f;
            let idx = if idx >= self.loop_len { idx - self.loop_len } else { idx };

            let (mut left, mut right) = (0.0f32, 0.0f32);
            for (l, layer) in self.layers.iter().enumerate() {
                let (p, c) = (self.prev_gains[l], self.cur_gains[l]);
                // A layer that is silent and staying silent costs nothing.
                if p <= 0.0001 && c <= 0.0001 {
                    continue;
                }
                let g = p + (c - p) * t;
                left += layer.at(idx, 0) as f32 * g;
                right += layer.at(idx, 1) as f32 * g;
            }

            let clamp = |x: f32| {
                (x * self.music_gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16
            };
            out[f * 2] = clamp(left);
            out[f * 2 + 1] = clamp(right);
        }

        self.playhead = (self.playhead + frames) % self.loop_len;

        if !sfx_muted {
            self.mix_voices(out, frames);
        } else {
            self.voices.clear();
        }
        self.frames += frames as u64;
    }

    fn mix_voices(&mut self, out: &mut [i16], frames: usize) {
        let mut i = 0;
        while i < self.voices.len() {
            let (sfx_idx, mut pos) = (self.voices[i].sfx, self.voices[i].pos);
            let spec = &self.sfx[sfx_idx];
            let n = spec.samples.frames();
            let gain = spec.gain * self.sfx_gain;

            for f in 0..frames {
                if pos >= n {
                    break;
                }
                for ch in 0..2 {
                    let v = spec.samples.at(pos, ch) as f32 * gain;
                    let o = &mut out[f * 2 + ch];
                    let sum = (*o as i32 + v as i32).clamp(i16::MIN as i32, i16::MAX as i32);
                    *o = sum as i16;
                }
                pos += 1;
            }

            if pos >= n {
                self.voices.remove(i);
            } else {
                self.voices[i].pos = pos;
                i += 1;
            }
        }
    }
}
