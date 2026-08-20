//! Loading and saving [`Settings`]: JSON in a per-OS config dir.
//!
//! The core deliberately has no file access, so this is the whole of the
//! platform's persistence story - and in a browser there is no story at all
//! yet. The stubs at the bottom keep the call sites identical; settings there
//! last as long as the tab does.

use erect_core::settings::Settings;
#[cfg(not(target_arch = "wasm32"))]
use erect_core::settings::PlayerConfig;
#[cfg(not(target_arch = "wasm32"))]
use directories::ProjectDirs;
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
/// On-disk shape, kept separate from the core type so the save format can
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

#[cfg(not(target_arch = "wasm32"))]
/// Missing from an older save file means the setting did not exist yet, and
/// full volume is what the player was hearing at the time.
fn full_volume() -> u32 {
    100
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> Settings {
    let Some(stored) = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<StoredSettings>(&text).ok())
    else {
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

#[cfg(not(target_arch = "wasm32"))]
pub fn save(settings: &Settings) {
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
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
    if let Ok(text) = serde_json::to_string_pretty(&stored) {
        // Losing settings is not worth interrupting play over.
        let _ = std::fs::write(path, text);
    }
}

/* ---------------- browser ---------------- */

/// A browser tab has no filesystem, and the shim that would give it one
/// (localStorage through a JS binding) is not worth pulling in before the build
/// has been played once. Settings still work for the length of the session -
/// the core holds them - they simply do not outlive the tab.
#[cfg(target_arch = "wasm32")]
pub fn load() -> Settings {
    Settings::default()
}

#[cfg(target_arch = "wasm32")]
pub fn save(_settings: &Settings) {}
