//! Platform-free game logic for E-Rect.
//!
//! This crate knows nothing about windows, GPUs, gamepads or filesystems. It
//! owns the simulation, wave pacing, menus and the *data* for settings, and it
//! is `no_std` so the same code runs on a desktop and on a PSP.
//!
//! Frontends supply three things:
//!
//! * an [`InputFrame`](input::InputFrame) each tick,
//! * the list of [`SchemeInfo`](settings::SchemeInfo) control schemes the
//!   platform offers (a desktop has keyboards and pads, a PSP has one pad),
//! * loading and saving of [`Settings`](settings::Settings), since file access
//!   differs everywhere.
//!
//! and then draw whatever the public state says.

#![no_std]

extern crate alloc;

pub mod audio;
pub mod backdrop;
pub mod color;
pub mod config;
pub mod dev;
pub mod entities;
pub mod game;
pub mod geom;
pub mod input;
pub mod menu;
pub mod settings;
pub mod waves;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

pub use audio::{AudioEvent, AudioQueue, MusicState, Situation};
pub use backdrop::{visible_blocks, BackdropBlock};
pub use color::{EaseColor, Rgb};
pub use dev::DevSetup;
pub use game::{Game, RunResult, State};
pub use geom::{Body, Viewport};
pub use input::{InputFrame, MenuIntent, PlayerIntent};
pub use settings::{SchemeInfo, Settings, VolumeChannel};
pub use waves::WaveKind;
