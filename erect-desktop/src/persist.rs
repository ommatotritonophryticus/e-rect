//! Loading and saving [`Settings`].
//!
//! The core deliberately has no file access, so this is the whole of the
//! platform's persistence story. Two stores, one format: a JSON file in a
//! per-OS config directory on a desktop, and the same JSON under one
//! localStorage key in a browser. Only the two functions at the very bottom of
//! each half differ.

use erect_core::settings::PlayerConfig;
use erect_core::settings::Settings;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use directories::ProjectDirs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// The stored shape, kept separate from the core type so the save format can
/// change without the simulation caring.
#[derive(Serialize, Deserialize)]
struct StoredSettings {
    players: Vec<StoredPlayer>,
    #[serde(default)]
    record_solo: i64,
    #[serde(default)]
    record_duo: i64,
    #[serde(default = "full_volume")]
    music_volume: u32,
    #[serde(default = "full_volume")]
    sfx_volume: u32,
    /// Only there to keep the seed moving; see `Settings::launches`.
    #[serde(default)]
    launches: u64,
}

/// Missing from an older save file means the setting did not exist yet, and
/// full volume is what the player was hearing at the time.
fn full_volume() -> u32 {
    100
}

#[derive(Serialize, Deserialize)]
struct StoredPlayer {
    scheme: usize,
    color_index: usize,
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "erect")?;
    Some(dirs.config_dir().join("settings.json"))
}

/// Text to settings. Shared, so the browser and the desktop can never drift
/// into reading the same file differently.
fn parse(text: &str) -> Settings {
    let Ok(stored) = serde_json::from_str::<StoredSettings>(text) else {
        return Settings::default();
    };
    let mut settings = Settings::default();
    for (i, player) in stored.players.iter().take(settings.players.len()).enumerate() {
        settings.players[i] = PlayerConfig {
            scheme: player.scheme,
            color_index: player.color_index,
        };
    }
    settings.record_solo = stored.record_solo;
    settings.record_duo = stored.record_duo;
    settings.music_volume = stored.music_volume;
    settings.sfx_volume = stored.sfx_volume;
    settings.launches = stored.launches;
    settings.dirty = false;
    // Game::new sanitizes this against the real scheme count.
    settings
}

/// Settings to text. The other half of [`parse`].
fn encode(settings: &Settings) -> Option<String> {
    let stored = StoredSettings {
        players: settings
            .players
            .iter()
            .map(|p| StoredPlayer {
                scheme: p.scheme,
                color_index: p.color_index,
            })
            .collect(),
        record_solo: settings.record_solo,
        record_duo: settings.record_duo,
        music_volume: settings.music_volume,
        sfx_volume: settings.sfx_volume,
        launches: settings.launches,
    };
    serde_json::to_string(&stored).ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> Settings {
    match path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => parse(&text),
        None => Settings::default(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save(settings: &Settings) {
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Some(text) = encode(settings) {
        // Losing settings is not worth interrupting play over.
        let _ = std::fs::write(path, text);
    }
}

/* ---------------- browser ---------------- */

// Written by `web/erect_web.js`, which registers them as a miniquad plugin and
// keeps the whole settings document under one localStorage key.
//
// Strings cross as a pointer and a length into the wasm heap; there is no
// allocator on the JS side of this boundary and nothing is retained past the
// call. `read` is handed a buffer to fill and answers how much it used, so the
// Rust side owns every byte involved.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn erect_storage_write(ptr: *const u8, len: u32);
    fn erect_storage_read(ptr: *mut u8, cap: u32) -> u32;
}

/// Room for the settings document. It is a handful of small integers and two
/// fields per player; a kilobyte is an order of magnitude more than it can be.
#[cfg(target_arch = "wasm32")]
const STORAGE_CAP: usize = 1024;

#[cfg(target_arch = "wasm32")]
pub fn load() -> Settings {
    let mut buf = alloc_buf();
    // A browser can refuse storage outright - private windows do - and it can
    // hold something older or hand-edited. Every one of those means "no
    // settings yet", which is what a first run looks like anyway.
    let len = unsafe { erect_storage_read(buf.as_mut_ptr(), STORAGE_CAP as u32) } as usize;
    if len == 0 || len > STORAGE_CAP {
        return Settings::default();
    }
    buf.truncate(len);
    match core::str::from_utf8(&buf) {
        Ok(text) => parse(text),
        Err(_) => Settings::default(),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn save(settings: &Settings) {
    if let Some(text) = encode(settings) {
        unsafe { erect_storage_write(text.as_ptr(), text.len() as u32) };
    }
}

#[cfg(target_arch = "wasm32")]
fn alloc_buf() -> Vec<u8> {
    vec![0u8; STORAGE_CAP]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact document a browser was found holding, byte for byte.
    const STORED: &str = r#"{"players":[{"scheme":0,"color_index":0},{"scheme":1,"color_index":1}],"record_solo":0,"record_duo":0,"music_volume":100,"sfx_volume":100,"launches":1}"#;

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let mut settings = Settings::default();
        settings.record_solo = 4321;
        settings.music_volume = 40;
        settings.launches = 7;
        let text = encode(&settings).expect("settings should encode");
        let back = parse(&text);
        assert_eq!(back.record_solo, 4321);
        assert_eq!(back.music_volume, 40);
        assert_eq!(back.launches, 7);
        assert!(!back.dirty, "a freshly loaded document has nothing to save");
    }

    #[test]
    fn a_document_from_a_real_browser_parses() {
        let settings = parse(STORED);
        assert_eq!(settings.launches, 1, "the launch counter should survive");
        assert_eq!(settings.music_volume, 100);
    }

    #[test]
    fn nonsense_reads_as_a_first_run_rather_than_failing() {
        assert_eq!(parse("").launches, 0);
        assert_eq!(parse("{").launches, 0);
        assert_eq!(parse("[1,2,3]").launches, 0);
    }

    #[test]
    fn the_document_fits_the_buffer_the_browser_is_given() {
        // The browser side refuses to write past the buffer, so a document that
        // outgrew it would silently stop loading.
        let mut settings = Settings::default();
        settings.record_solo = i64::MAX;
        settings.record_duo = i64::MAX;
        settings.launches = u64::MAX;
        let text = encode(&settings).expect("settings should encode");
        assert!(text.len() < 1024, "the document is {} bytes", text.len());
    }
}
