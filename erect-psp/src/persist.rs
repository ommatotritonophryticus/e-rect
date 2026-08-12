//! Settings on the Memory Stick, via sceIo.
//!
//! A fixed-size binary record rather than JSON: no parser to carry, and a
//! corrupt or truncated file simply falls back to defaults.

use erect_core::settings::Settings;
use psp::sys::{self, IoOpenFlags};

const PATH: &[u8] = b"ms0:/PSP/GAME/ERECT/settings.dat\0";
const DIR: &[u8] = b"ms0:/PSP/GAME/ERECT\0";
/// Bumped whenever the layout below changes, so old files are ignored.
const MAGIC: u32 = 0x4552_4303;
const RECORD_LEN: usize = 4 + 4 + 4 + 8 + 8 + 4 + 4 + 8;

pub fn load() -> Settings {
    let mut settings = Settings::default();
    let mut buf = [0u8; RECORD_LEN];

    unsafe {
        let fd = sys::sceIoOpen(PATH.as_ptr(), IoOpenFlags::RD_ONLY, 0o777);
        if fd.0 < 0 {
            return settings;
        }
        let read = sys::sceIoRead(fd, buf.as_mut_ptr() as *mut _, RECORD_LEN as u32);
        sys::sceIoClose(fd);
        if read < RECORD_LEN as i32 {
            return settings;
        }
    }

    if u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) != MAGIC {
        return settings;
    }
    settings.players[0].color_index =
        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    settings.players[1].color_index =
        u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    settings.record_solo = i64::from_le_bytes([
        buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19],
    ]);
    settings.record_duo = i64::from_le_bytes([
        buf[20], buf[21], buf[22], buf[23], buf[24], buf[25], buf[26], buf[27],
    ]);
    settings.music_volume = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
    settings.sfx_volume = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
    settings.launches = u64::from_le_bytes([
        buf[36], buf[37], buf[38], buf[39], buf[40], buf[41], buf[42], buf[43],
    ]);
    settings.dirty = false;
    // Game::new sanitizes whatever came off the stick.
    settings
}

pub fn save(settings: &Settings) {
    let mut buf = [0u8; RECORD_LEN];
    buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&(settings.players[0].color_index as u32).to_le_bytes());
    buf[8..12].copy_from_slice(&(settings.players[1].color_index as u32).to_le_bytes());
    buf[12..20].copy_from_slice(&settings.record_solo.to_le_bytes());
    buf[20..28].copy_from_slice(&settings.record_duo.to_le_bytes());
    buf[28..32].copy_from_slice(&settings.music_volume.to_le_bytes());
    buf[32..36].copy_from_slice(&settings.sfx_volume.to_le_bytes());
    buf[36..44].copy_from_slice(&settings.launches.to_le_bytes());

    unsafe {
        // The directory may not exist on a fresh stick; failure is fine.
        sys::sceIoMkdir(DIR.as_ptr(), 0o777);
        let fd = sys::sceIoOpen(
            PATH.as_ptr(),
            IoOpenFlags::CREAT | IoOpenFlags::TRUNC | IoOpenFlags::WR_ONLY,
            0o777,
        );
        if fd.0 >= 0 {
            sys::sceIoWrite(fd, buf.as_ptr() as *const _, RECORD_LEN);
            sys::sceIoClose(fd);
        }
    }
}
