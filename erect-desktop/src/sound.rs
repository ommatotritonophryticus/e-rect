//! Desktop audio: decode the pack, run the shared mixer, push to the device.
//!
//! macroquad's own audio API plays each sound as an independent source, which
//! cannot hold six layers in sample-accurate sync. So the same software mixer
//! the PSP uses runs here too, and cpal only carries the finished blocks.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use erect_audio::packs;
use erect_audio::{GainEngine, Mixer, Samples, SfxSpec};
use erect_core::audio::{AudioEvent, MusicState, Situation};

/// Decoded pack data, owned here and borrowed by the mixer on the audio thread.
struct PackData {
    layers: Vec<Vec<i16>>,
    sfx: Vec<(AudioEvent, Vec<i16>, f32, usize, f32)>,
}

/// Every shipped pack, decoded. Both are held because the soundtrack changes on
/// arriving at the menu and on starting a run: re-reading one off disk at those
/// moments would stall the game exactly when it should be responsive.
struct AllPacks {
    packs: Vec<PackData>,
}

/// Returns interleaved stereo, duplicating mono sources, so the mixer always
/// sees the same shape and the composition keeps the width it was written with.
fn decode_flac(path: &Path) -> Result<Vec<i16>, String> {
    let mut reader = claxon::FlacReader::open(path).map_err(|e| format!("{path:?}: {e}"))?;
    let channels = reader.streaminfo().channels as usize;
    let shift = reader.streaminfo().bits_per_sample as i32 - 16;

    let mut out = Vec::new();
    let mut frame: Vec<i16> = Vec::with_capacity(channels);
    for sample in reader.samples() {
        let s = sample.map_err(|e| format!("{path:?}: {e}"))?;
        let s = if shift > 0 { s >> shift } else { s << (-shift) };
        frame.push(s.clamp(-32768, 32767) as i16);
        if frame.len() == channels {
            match channels {
                1 => {
                    out.push(frame[0]);
                    out.push(frame[0]);
                }
                _ => {
                    out.push(frame[0]);
                    out.push(frame[1]);
                }
            }
            frame.clear();
        }
    }
    Ok(out)
}

fn load_pack(dir: &Path, spec: &packs::Pack) -> Result<PackData, String> {
    let mut layers = Vec::new();
    for id in packs::LAYER_IDS {
        layers.push(decode_flac(&dir.join(format!("{id}.flac")))?);
    }
    let mut sfx = Vec::new();
    for (id, max_voices, min_ms, _psp_db) in spec.sfx {
        let event = match id {
            "hit" => AudioEvent::Hit,
            "jump" => AudioEvent::Jump,
            "down" => AudioEvent::Slam,
            other => return Err(format!("unknown sfx id {other}")),
        };
        sfx.push((event, decode_flac(&dir.join(format!("{id}.flac")))?, 1.0, max_voices, min_ms));
    }
    Ok(PackData { layers, sfx })
}

/// What the game tells the audio thread each frame.
#[derive(Clone, Copy)]
enum Msg {
    State(MusicState),
    Fire(AudioEvent),
    Volumes { music: u32, sfx: u32 },
    /// The core rolled a new soundtrack.
    Roll(u64),
}

pub struct Sound {
    tx: Sender<Msg>,
    /// Held so the device stays open for as long as the game runs.
    _stream: cpal::Stream,
}

impl Sound {
    /// Returns `None` (with a reason) rather than failing the game: no sound
    /// card is a poor reason not to be able to play.
    /// `seed` picks the opening soundtrack; the core rolls again later.
    pub fn start(packs_root: &Path, seed: u64) -> Result<Self, String> {
        let mut decoded = Vec::new();
        for spec in packs::PACKS.iter() {
            decoded.push(load_pack(&packs_root.join(spec.dir).join("desktop"), spec)?);
        }
        let data = AllPacks { packs: decoded };

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no output device".to_string())?;
        let default = device
            .default_output_config()
            .map_err(|e| format!("no output config: {e}"))?;

        // Running the device at the pack's own rate avoids resampling entirely,
        // so ask for it before settling for whatever the system prefers.
        let channels = default.channels();
        let native = device
            .supported_output_configs()
            .ok()
            .and_then(|mut it| {
                it.find(|c| {
                    c.channels() == channels
                        && c.min_sample_rate().0 <= packs::SAMPLE_RATE
                        && c.max_sample_rate().0 >= packs::SAMPLE_RATE
                        && c.sample_format() == cpal::SampleFormat::F32
                })
            })
            .map(|c| c.with_sample_rate(cpal::SampleRate(packs::SAMPLE_RATE)));

        let config: cpal::StreamConfig = match native {
            Some(c) => c.into(),
            None => default.into(),
        };
        let out_rate = config.sample_rate.0;
        let out_channels = config.channels as usize;

        let (tx, rx) = mpsc::channel::<Msg>();
        let shared = Arc::new(Mutex::new(data));
        let stream = build_stream(&device, &config, shared, rx, out_channels, out_rate, seed)?;
        stream.play().map_err(|e| format!("cannot start stream: {e}"))?;

        Ok(Self { tx, _stream: stream })
    }

    pub fn set_state(&self, state: MusicState) {
        let _ = self.tx.send(Msg::State(state));
    }

    pub fn fire(&self, event: AudioEvent) {
        let _ = self.tx.send(Msg::Fire(event));
    }

    pub fn set_volumes(&self, music: u32, sfx: u32) {
        let _ = self.tx.send(Msg::Volumes { music, sfx });
    }

    pub fn set_roll(&self, roll: u64) {
        let _ = self.tx.send(Msg::Roll(roll));
    }
}

/// Reads the mixer's 44.1 kHz output at whatever rate the device wants.
///
/// Linear interpolation: for a device that is almost always either 44.1 or 48
/// kHz, the error sits far above anything in this music.
struct Resampler {
    ratio: f64,
    buf: Vec<i16>,
    pos: f64,
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            ratio: in_rate as f64 / out_rate as f64,
            buf: Vec::new(),
            pos: 0.0,
        }
    }

    /// Fills `out` (interleaved, `out_channels` wide) from the mixer.
    fn convert(
        &mut self,
        out: &mut [f32],
        out_channels: usize,
        mut render: impl FnMut(&mut Vec<i16>, usize),
    ) {
        let frames = out.len() / out_channels;
        let span = self.pos + frames as f64 * self.ratio;
        let needed = span.ceil() as usize + 2;
        while self.buf.len() / 2 < needed {
            let want = needed - self.buf.len() / 2;
            render(&mut self.buf, want);
        }

        for f in 0..frames {
            let p = self.pos + f as f64 * self.ratio;
            let i = p.floor() as usize;
            let t = (p - i as f64) as f32;
            let lerp = |a: i16, b: i16| (a as f32 + (b as f32 - a as f32) * t) / 32768.0;
            let l = lerp(self.buf[i * 2], self.buf[(i + 1) * 2]);
            let r = lerp(self.buf[i * 2 + 1], self.buf[(i + 1) * 2 + 1]);
            for ch in 0..out_channels {
                out[f * out_channels + ch] = if ch % 2 == 0 { l } else { r };
            }
        }

        let consumed = span.floor() as usize;
        self.buf.drain(..consumed * 2);
        self.pos = span - consumed as f64;
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    data: Arc<Mutex<AllPacks>>,
    rx: Receiver<Msg>,
    out_channels: usize,
    out_rate: u32,
    seed: u64,
) -> Result<cpal::Stream, String> {
    // The mixer borrows the sample data, so the data has to outlive the
    // callback. Leaking it once at startup is simpler and cheaper than an Arc
    // dance on every block, and it lives until the process exits anyway.
    let owned: &'static AllPacks = Box::leak(Box::new(
        Arc::try_unwrap(data)
            .map_err(|_| "pack still shared".to_string())?
            .into_inner()
            .map_err(|e| format!("pack lock poisoned: {e}"))?,
    ));

    // Effects come from the first pack: all of them ship the same three sounds,
    // so a swap never has to touch them.
    let music_of = |i: usize| -> Vec<Samples<'static>> {
        owned.packs[i]
            .layers
            .iter()
            .map(|v| Samples::I16Stereo(v))
            .collect()
    };
    let index_of = |roll: u64| -> usize {
        let picked = packs::choose(roll);
        packs::PACKS.iter().position(|p| p.dir == picked.dir).unwrap_or(0)
    };

    let mut current = index_of(seed);
    let sfx: Vec<SfxSpec<'static>> = owned.packs[0]
        .sfx
        .iter()
        .map(|(event, samples, gain, max_voices, min_ms)| SfxSpec {
            event: *event,
            samples: Samples::I16Stereo(samples),
            gain: *gain,
            max_voices: *max_voices,
            min_interval_frames: (*min_ms * packs::SAMPLE_RATE as f32 / 1000.0) as u64,
        })
        .collect();

    let gains = GainEngine::new(packs::layers(packs::PACKS[current].desktop_gains_db));
    let mut mixer = Mixer::new(music_of(current), gains, sfx, packs::SAMPLE_RATE);
    let mut state = MusicState {
        situation: Situation::Calm,
        zombies: 0,
        flyers: 0,
        boss: false,
    };
    let mut resampler = Resampler::new(packs::SAMPLE_RATE, out_rate);
    let mut block: Vec<i16> = Vec::new();

    device
        .build_output_stream(
            config,
            move |out: &mut [f32], _| {
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        Msg::State(s) => state = s,
                        Msg::Fire(e) => mixer.fire(e),
                        Msg::Volumes { music, sfx } => mixer.set_volumes(music, sfx),
                        Msg::Roll(roll) => {
                            let want = index_of(roll);
                            if want != current {
                                current = want;
                                mixer.switch_music(
                                    music_of(current),
                                    GainEngine::new(packs::layers(
                                        packs::PACKS[current].desktop_gains_db,
                                    )),
                                );
                            }
                        }
                    }
                }
                let muted = state.situation == Situation::Paused;
                resampler.convert(out, out_channels, |buf, want| {
                    block.resize(want * 2, 0);
                    mixer.render(&mut block, &state, muted);
                    buf.extend_from_slice(&block);
                });
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| format!("cannot build stream: {e}"))
}

#[cfg(test)]
mod probe {
    //! Renders a scripted match through the real pack and the real mixer, so
    //! the whole chain can be checked without a sound card. The output WAV is
    //! left in `target/` for inspection.

    use super::*;
    use erect_audio::{GainEngine, Mixer, Samples, SfxSpec};
    use std::io::Write;

    #[test]
    fn renders_a_scripted_match_to_a_wav() {
        let spec = &packs::PACKS[0];
        let dir = super::super::packs_dir().join(spec.dir).join("desktop");
        if !dir.is_dir() {
            eprintln!("pack not built, skipping: {dir:?}");
            return;
        }
        let data = load_pack(&dir, spec).expect("pack should load");
        let layers: Vec<Samples> = data.layers.iter().map(|v| Samples::I16Stereo(v)).collect();
        let sfx: Vec<SfxSpec> = data
            .sfx
            .iter()
            .map(|(event, s, gain, mv, ms)| SfxSpec {
                event: *event,
                samples: Samples::I16Stereo(s),
                gain: *gain,
                max_voices: *mv,
                min_interval_frames: (*ms * packs::SAMPLE_RATE as f32 / 1000.0) as u64,
            })
            .collect();

        let gains = GainEngine::new(packs::layers(spec.desktop_gains_db));
        let mut mixer = Mixer::new(layers, gains, sfx, packs::SAMPLE_RATE);

        // seconds -> what the game is doing at that point
        let script: &[(f32, MusicState, bool)] = &[
            (3.0, st(Situation::Calm, 0, 0, false), false),
            (3.0, st(Situation::Combat, 0, 0, false), true),
            (3.0, st(Situation::Combat, 2, 0, false), true),
            (3.0, st(Situation::Combat, 4, 3, false), true),
            (3.0, st(Situation::Combat, 4, 3, true), false),
            (3.0, st(Situation::Paused, 4, 3, true), false),
            (3.0, st(Situation::Calm, 0, 0, false), false),
        ];

        let block = 1024;
        let mut buf = vec![0i16; block * 2];
        let mut out = Vec::new();
        for (secs, state, fire) in script {
            let blocks = (*secs * packs::SAMPLE_RATE as f32 / block as f32) as usize;
            for b in 0..blocks {
                if *fire && b % 20 == 0 {
                    mixer.fire(AudioEvent::Hit);
                }
                if *fire && b % 53 == 0 {
                    mixer.fire(AudioEvent::Jump);
                }
                let muted = state.situation == Situation::Paused;
                mixer.render(&mut buf, state, muted);
                out.extend_from_slice(&buf);
            }
        }

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/audio_probe.wav");
        write_wav(&path, &out, packs::SAMPLE_RATE);
        eprintln!("wrote {path:?} ({} frames)", out.len() / 2);

        let peak = out.iter().map(|s| s.unsigned_abs() as u32).max().unwrap_or(0);
        assert!(peak > 3000, "output is far too quiet: peak {peak}");
        assert!(peak < 32767, "output is clipping: peak {peak}");
    }

    /// The PSP runs the same mixer over 8-bit data with different baked gains.
    /// If those two paths disagree, the PSP quietly ships a different mix - so
    /// the check is that the two agree scene by scene.
    #[test]
    fn psp_encoding_reproduces_the_desktop_mix() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../packs")
            .join(packs::PACKS[0].dir);
        if !root.join("psp").is_dir() {
            eprintln!("pack not built, skipping");
            return;
        }

        let scenes: &[MusicState] = &[
            st(Situation::Calm, 0, 0, false),
            st(Situation::Combat, 0, 0, false),
            st(Situation::Combat, 2, 0, false),
            st(Situation::Combat, 4, 3, false),
            st(Situation::Combat, 4, 3, true),
            st(Situation::Paused, 4, 3, true),
        ];

        let desktop = render_levels(&root.join("desktop"), scenes, false);
        let psp = render_levels(&root.join("psp"), scenes, true);

        for (i, (d, p)) in desktop.iter().zip(psp.iter()).enumerate() {
            let diff = (d - p).abs();
            eprintln!("scene {i}: desktop {d:.1} dBFS   psp {p:.1} dBFS   diff {diff:.2}");
            assert!(
                diff < 1.5,
                "scene {i}: psp mix is {diff:.2} dB off the desktop mix"
            );
        }
    }

    /// Per-scene RMS in dBFS, rendered through the real mixer.
    fn render_levels(dir: &std::path::Path, scenes: &[MusicState], psp: bool) -> Vec<f32> {
        let layers_raw: Vec<Vec<i16>> = packs::LAYER_IDS
            .iter()
            .map(|id| {
                if psp {
                    read_u8_wav(&dir.join(format!("{id}.wav")))
                } else {
                    decode_flac(&dir.join(format!("{id}.flac"))).expect("flac")
                }
            })
            .collect();
        let layers: Vec<Samples> = layers_raw
            .iter()
            .map(|v| if psp { Samples::I16Mono(v) } else { Samples::I16Stereo(v) })
            .collect();
        let spec = &packs::PACKS[0];
        let gains = GainEngine::new(packs::layers(if psp {
            spec.psp_gains_db
        } else {
            spec.desktop_gains_db
        }));
        let mut mixer = Mixer::new(layers, gains, Vec::new(), packs::SAMPLE_RATE);

        let mut out = Vec::new();
        let mut buf = vec![0i16; 1024 * 2];
        for scene in scenes {
            let mut acc = 0.0f64;
            let mut n = 0usize;
            for _ in 0..(packs::SAMPLE_RATE as usize * 2 / 1024) {
                mixer.render(&mut buf, scene, scene.situation == Situation::Paused);
                // Both channels, so a stereo mix is not scored as if it were mono.
                for s in buf.iter() {
                    acc += (*s as f64 / 32768.0).powi(2);
                    n += 1;
                }
            }
            out.push((20.0 * (acc / n as f64).sqrt().max(1e-12).log10()) as f32);
        }
        out
    }

    /// Reads the 8-bit mono WAV the PSP pack ships, widened to i16 exactly as
    /// the mixer does on the device.
    fn read_u8_wav(path: &std::path::Path) -> Vec<i16> {
        let raw = std::fs::read(path).expect("psp wav");
        let mut p = 12usize;
        while p + 8 <= raw.len() {
            let len = u32::from_le_bytes(raw[p + 4..p + 8].try_into().unwrap()) as usize;
            if &raw[p..p + 4] == b"data" {
                let end = (p + 8 + len).min(raw.len());
                return raw[p + 8..end]
                    .iter()
                    .map(|b| ((*b as i32 - 128) << 8) as i16)
                    .collect();
            }
            p += 8 + len + (len & 1);
        }
        panic!("no data chunk in {path:?}");
    }

    fn st(situation: Situation, zombies: usize, flyers: usize, boss: bool) -> MusicState {
        MusicState { situation, zombies, flyers, boss }
    }

    fn write_wav(path: &std::path::Path, samples: &[i16], rate: u32) {
        let bytes = samples.len() * 2;
        let mut f = std::fs::File::create(path).expect("cannot write probe wav");
        let hdr_tail = (36 + bytes) as u32;
        f.write_all(b"RIFF").unwrap();
        f.write_all(&hdr_tail.to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * 4).to_le_bytes()).unwrap();
        f.write_all(&4u16.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&(bytes as u32).to_le_bytes()).unwrap();
        for s in samples {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
    }
}
