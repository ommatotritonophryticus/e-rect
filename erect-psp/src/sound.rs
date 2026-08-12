//! PSP audio: load the pack off the Memory Stick, mix on a dedicated thread.
//!
//! `sceAudioOutputBlocking` sleeps until the hardware has taken the buffer,
//! which is exactly what an audio thread wants and exactly what the vsync-locked
//! render loop must not do - a 1024-sample block is 23 ms against a 16.7 ms
//! frame, so mixing on the main thread would drag the game down to 43 fps.
//!
//! The two threads share state through atomics rather than a lock: the whole
//! music state packs into one word, and one-shot cues are just counters.

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

use erect_audio::packs;
use erect_audio::{GainEngine, Mixer, Samples, SfxSpec};
use erect_core::audio::{AudioEvent, MusicState, Situation};
use psp::sys::{self, AudioFormat, IoOpenFlags, ThreadAttributes};

/// Multiple of 64, as the hardware requires. 1024 frames is 23 ms of audio.
const BLOCK: usize = 1024;
const PACK_ROOT: &[u8] = b"ms0:/PSP/GAME/ERECT/pack/";

/// `situation | zombies | flyers | boss`, packed so a single atomic carries it.
static MUSIC_STATE: AtomicU32 = AtomicU32::new(0);
/// One counter per cue; the audio thread takes and clears them.
static CUE_HIT: AtomicU32 = AtomicU32::new(0);
static CUE_JUMP: AtomicU32 = AtomicU32::new(0);
static CUE_SLAM: AtomicU32 = AtomicU32::new(0);
/// `music | sfx << 8`, both 0..=100.
static VOLUMES: AtomicU32 = AtomicU32::new(100 | (100 << 8));
/// Which pack the mixer should be playing. The core re-rolls on arriving at the
/// menu and on starting a run; both are already loaded, so the swap is free.
static WANT_PACK: AtomicU32 = AtomicU32::new(0);
/// Free bytes left after the packs were loaded, for the title screen to show.
static FREE_AFTER_LOAD: AtomicU32 = AtomicU32::new(0);
/// Why there is no sound, if there is none. Silence used to be indistinguishable
/// from a missing pack, a refused channel or a dead thread, which left nothing
/// to go on when a build came up quiet on someone else's machine.
static STATUS: AtomicU32 = AtomicU32::new(SoundStatus::Starting as u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundStatus {
    Starting = 0,
    Playing = 1,
    /// A file under `ms0:/PSP/GAME/ERECT/pack/` could not be opened or read.
    NoPack = 2,
    /// `sceAudioChReserve` refused; every channel was already taken.
    NoChannel = 3,
    /// The audio thread could not be created.
    NoThread = 4,
}

pub fn status() -> SoundStatus {
    match STATUS.load(Ordering::Relaxed) {
        1 => SoundStatus::Playing,
        2 => SoundStatus::NoPack,
        3 => SoundStatus::NoChannel,
        4 => SoundStatus::NoThread,
        _ => SoundStatus::Starting,
    }
}

impl SoundStatus {
    /// Short enough for the title screen, specific enough to act on.
    pub fn message(self) -> Option<&'static str> {
        match self {
            SoundStatus::Starting | SoundStatus::Playing => None,
            SoundStatus::NoPack => Some("NO SOUND: PACK NOT FOUND ON MEMORY STICK"),
            SoundStatus::NoChannel => Some("NO SOUND: NO FREE AUDIO CHANNEL"),
            SoundStatus::NoThread => Some("NO SOUND: AUDIO THREAD FAILED"),
        }
    }
}

fn pack_state(s: &MusicState) -> u32 {
    let situation = match s.situation {
        Situation::Calm => 0u32,
        Situation::Combat => 1,
        Situation::Paused => 2,
    };
    situation | ((s.zombies.min(255) as u32) << 2) | ((s.flyers.min(255) as u32) << 10)
        | ((s.boss as u32) << 18)
}

fn unpack_state(v: u32) -> MusicState {
    MusicState {
        situation: match v & 3 {
            0 => Situation::Calm,
            1 => Situation::Combat,
            _ => Situation::Paused,
        },
        zombies: ((v >> 2) & 0xff) as usize,
        flyers: ((v >> 10) & 0xff) as usize,
        boss: (v >> 18) & 1 == 1,
    }
}

/// Called from the game loop each frame.
pub fn set_state(state: &MusicState) {
    MUSIC_STATE.store(pack_state(state), Ordering::Relaxed);
}

/// Called from the game loop; `roll` is whatever the core last drew.
pub fn set_roll(roll: u64) {
    let picked = packs::choose(roll);
    let idx = packs::PACKS.iter().position(|p| p.dir == picked.dir).unwrap_or(0);
    WANT_PACK.store(idx as u32, Ordering::Relaxed);
}

/// Bytes free once both packs were in memory. Zero before loading finishes.
pub fn free_after_load() -> u32 {
    FREE_AFTER_LOAD.load(Ordering::Relaxed)
}

pub fn set_volumes(music: u32, sfx: u32) {
    VOLUMES.store((music.min(100)) | (sfx.min(100) << 8), Ordering::Relaxed);
}

pub fn fire(event: AudioEvent) {
    let counter = match event {
        AudioEvent::Hit => &CUE_HIT,
        AudioEvent::Jump => &CUE_JUMP,
        AudioEvent::Slam => &CUE_SLAM,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Reads a mono 8-bit PCM WAV from `<root>/<pack>/<name>.wav`.
/// Only the shape `build_pack.py` writes.
fn read_wav(pack_dir: &str, name: &str) -> Option<Vec<u8>> {
    let mut path = [0u8; 128];
    let mut n = 0;
    for &b in PACK_ROOT {
        path[n] = b;
        n += 1;
    }
    for &b in pack_dir.as_bytes() {
        path[n] = b;
        n += 1;
    }
    path[n] = b'/';
    n += 1;
    for &b in name.as_bytes() {
        path[n] = b;
        n += 1;
    }
    for &b in b".wav\0" {
        path[n] = b;
        n += 1;
    }

    unsafe {
        let fd = sys::sceIoOpen(path.as_ptr(), IoOpenFlags::RD_ONLY, 0o777);
        if fd.0 < 0 {
            return None;
        }
        let size = sys::sceIoLseek(fd, 0, sys::IoWhence::End);
        sys::sceIoLseek(fd, 0, sys::IoWhence::Set);
        if size <= 44 {
            sys::sceIoClose(fd);
            return None;
        }
        // Read in a loop. `sceIoRead` is allowed to hand back less than asked
        // for, and these files are about a megabyte each; a single call that
        // came up short used to make the whole pack fail silently.
        let mut raw = vec![0u8; size as usize];
        let mut done = 0usize;
        while done < raw.len() {
            let got = sys::sceIoRead(
                fd,
                raw.as_mut_ptr().add(done) as *mut c_void,
                (raw.len() - done) as u32,
            );
            if got <= 0 {
                break;
            }
            done += got as usize;
        }
        sys::sceIoClose(fd);
        if done < raw.len() {
            return None;
        }

        // Walk the RIFF chunks to find `data`; the header is not always 44 bytes.
        let mut p = 12usize;
        while p + 8 <= raw.len() {
            let id = &raw[p..p + 4];
            let len = u32::from_le_bytes([raw[p + 4], raw[p + 5], raw[p + 6], raw[p + 7]])
                as usize;
            if id == b"data" {
                let end = (p + 8 + len).min(raw.len());
                return Some(raw[p + 8..end].to_vec());
            }
            p += 8 + len + (len & 1);
        }
        None
    }
}

struct PackData {
    layers: Vec<Vec<u8>>,
    sfx: Vec<Vec<u8>>,
}

/// Every shipped pack, loaded once and never freed so the mixer can hold plain
/// references. Both are held because the soundtrack changes on reaching the menu
/// and on starting a run - re-reading six megabytes off the Memory Stick at
/// those moments would stall the game for seconds.
static mut PACKS_DATA: Option<Vec<PackData>> = None;

/// Starts the audio thread. Returns false when the pack is missing, in which
/// case the game simply runs silent.
///
/// # Safety
/// Call once, before the game loop.
pub unsafe fn start(seed: u64) -> bool {
    set_roll(seed);

    let mut all = Vec::new();
    for spec in packs::PACKS.iter() {
        let mut layers = Vec::new();
        for id in packs::LAYER_IDS {
            match read_wav(spec.dir, id) {
                Some(v) => layers.push(v),
                None => {
                    STATUS.store(SoundStatus::NoPack as u32, Ordering::Relaxed);
                    return false;
                }
            }
        }
        let mut sfx = Vec::new();
        for (id, _, _, _) in spec.sfx {
            match read_wav(spec.dir, id) {
                Some(v) => sfx.push(v),
                None => {
                    STATUS.store(SoundStatus::NoPack as u32, Ordering::Relaxed);
                    return false;
                }
            }
        }
        all.push(PackData { layers, sfx });
    }
    unsafe {
        FREE_AFTER_LOAD.store(sys::sceKernelMaxFreeMemSize() as u32, Ordering::Relaxed);
        PACKS_DATA = Some(all);

        let thread = sys::sceKernelCreateThread(
            b"erect_audio\0".as_ptr(),
            audio_thread,
            0x12,
            0x2000,
            ThreadAttributes::USER,
            core::ptr::null_mut(),
        );
        if thread.0 < 0 {
            STATUS.store(SoundStatus::NoThread as u32, Ordering::Relaxed);
            return false;
        }
        sys::sceKernelStartThread(thread, 0, core::ptr::null_mut());
    }
    true
}

unsafe extern "C" fn audio_thread(_args: usize, _argp: *mut c_void) -> i32 {
    let all = match unsafe { (*core::ptr::addr_of!(PACKS_DATA)).as_ref() } {
        Some(d) => d,
        None => return 0,
    };

    let music_of = |i: usize| -> Vec<Samples> {
        all[i].layers.iter().map(|v| Samples::U8Mono(v)).collect()
    };

    let mut current = WANT_PACK.load(Ordering::Relaxed) as usize;
    // Effects are the same three sounds in every pack, so a swap never has to
    // rebuild them.
    let sfx: Vec<SfxSpec> = packs::PACKS[0]
        .sfx
        .iter()
        .enumerate()
        .map(|(i, (id, max_voices, min_ms, gain_db))| SfxSpec {
            event: match *id {
                "hit" => AudioEvent::Hit,
                "jump" => AudioEvent::Jump,
                _ => AudioEvent::Slam,
            },
            samples: Samples::U8Mono(&all[0].sfx[i]),
            gain: erect_audio::gains::db_to_linear(*gain_db),
            max_voices: *max_voices,
            min_interval_frames: (*min_ms * packs::SAMPLE_RATE as f32 / 1000.0) as u64,
        })
        .collect();

    let gains = GainEngine::new(packs::layers(packs::PACKS[current].psp_gains_db));
    let mut mixer = Mixer::new(music_of(current), gains, sfx, packs::SAMPLE_RATE);

    let channel = unsafe {
        sys::sceAudioChReserve(-1, BLOCK as i32, AudioFormat::Stereo)
    };
    if channel < 0 {
        STATUS.store(SoundStatus::NoChannel as u32, Ordering::Relaxed);
        return 0;
    }
    STATUS.store(SoundStatus::Playing as u32, Ordering::Relaxed);

    let mut buf = vec![0i16; BLOCK * 2];
    let mut last_volumes = u32::MAX;
    loop {
        let want = WANT_PACK.load(Ordering::Relaxed) as usize;
        if want != current && want < all.len() {
            current = want;
            mixer.switch_music(
                music_of(current),
                GainEngine::new(packs::layers(packs::PACKS[current].psp_gains_db)),
            );
        }
        let volumes = VOLUMES.load(Ordering::Relaxed);
        if volumes != last_volumes {
            mixer.set_volumes(volumes & 0xff, (volumes >> 8) & 0xff);
            last_volumes = volumes;
        }
        let state = unpack_state(MUSIC_STATE.load(Ordering::Relaxed));
        let muted = state.situation == Situation::Paused;

        for (counter, event) in [
            (&CUE_HIT, AudioEvent::Hit),
            (&CUE_JUMP, AudioEvent::Jump),
            (&CUE_SLAM, AudioEvent::Slam),
        ] {
            let pending = counter.swap(0, Ordering::Relaxed).min(4);
            for _ in 0..pending {
                mixer.fire(event);
            }
        }

        mixer.render(&mut buf, &state, muted);
        unsafe {
            sys::sceAudioOutputBlocking(
                channel,
                sys::AUDIO_VOLUME_MAX as i32,
                buf.as_mut_ptr() as *mut c_void,
            );
        }
    }
}
