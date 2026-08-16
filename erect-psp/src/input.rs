//! sceCtrl, turned into the core's [`InputFrame`].
//!
//! The PSP has one controller, so there is exactly one scheme and only ever one
//! player.

use erect_core::config::GAMEPAD_DEADZONE;
use erect_core::input::{InputFrame, MenuIntent, PlayerIntent};
use erect_core::settings::SchemeInfo;
use psp::sys::{self, CtrlButtons, CtrlMode, SceCtrlData};

/// One pad, so one scheme. This is also what makes the core hide 2-player mode.
pub static SCHEMES: [SchemeInfo; 1] = [SchemeInfo {
    label: "PSP PAD",
    is_gamepad: true,
    pad_index: 0,
}];

#[derive(Clone, Copy, Default, PartialEq)]
struct PadState {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    cross: bool,
    square: bool,
    circle: bool,
    triangle: bool,
    start: bool,
    /// Only ever a modifier: nothing in the game itself is bound to it.
    select: bool,
}

pub struct Input {
    prev: PadState,
    cur: PadState,
}

impl Input {
    /// # Safety
    /// Initialises the controller driver; call once at startup.
    pub unsafe fn new() -> Self {
        unsafe {
            sys::sceCtrlSetSamplingCycle(0);
            sys::sceCtrlSetSamplingMode(CtrlMode::Analog);
        }
        Self {
            prev: PadState::default(),
            cur: PadState::default(),
        }
    }

    /// # Safety
    /// Reads the controller; call once per rendered frame.
    pub unsafe fn read(&mut self) -> InputFrame {
        let mut pad = SceCtrlData::default();
        unsafe {
            sys::sceCtrlReadBufferPositive(&mut pad, 1);
        }

        // The analog nub reports 0..=255 with 128 at rest.
        let nub_x = (pad.lx as f32 - 128.0) / 128.0;
        let b = pad.buttons;

        self.prev = self.cur;
        self.cur = PadState {
            left: b.contains(CtrlButtons::LEFT) || nub_x < -GAMEPAD_DEADZONE,
            right: b.contains(CtrlButtons::RIGHT) || nub_x > GAMEPAD_DEADZONE,
            up: b.contains(CtrlButtons::UP),
            down: b.contains(CtrlButtons::DOWN),
            cross: b.contains(CtrlButtons::CROSS),
            square: b.contains(CtrlButtons::SQUARE),
            circle: b.contains(CtrlButtons::CIRCLE),
            triangle: b.contains(CtrlButtons::TRIANGLE),
            start: b.contains(CtrlButtons::START),
            select: b.contains(CtrlButtons::SELECT),
        };

        let pressed = |now: bool, before: bool| now && !before;
        let c = self.cur;
        let p = self.prev;

        let mut frame = InputFrame {
            pads_connected: [true, false],
            ..Default::default()
        };

        frame.players[0] = PlayerIntent {
            left: c.left,
            right: c.right,
            jump: pressed(c.cross, p.cross),
            // Slam is Circle or d-pad down, whichever the player reaches for.
            slam: pressed(c.circle, p.circle) || pressed(c.down, p.down),
            attack: pressed(c.square, p.square),
            attack_held: c.square,
            dash: pressed(c.triangle, p.triangle),
        };

        frame.menu = MenuIntent {
            up: pressed(c.up, p.up),
            down: pressed(c.down, p.down),
            left: pressed(c.left, p.left),
            right: pressed(c.right, p.right),
            confirm: pressed(c.cross, p.cross) || pressed(c.square, p.square),
            back: pressed(c.circle, p.circle),
        };
        frame.pause = pressed(c.start, p.start);
        // Select held, then Down. A chord rather than a button, and Select is
        // not used for anything else, so it cannot be hit while playing.
        frame.dev_menu = c.select && pressed(c.down, p.down);

        frame
    }
}
