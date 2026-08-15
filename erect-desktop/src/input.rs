//! Keyboard and gamepad, collapsed into a per-frame snapshot of intents.
//!
//! The browser version was event-driven (keydown/keyup). macroquad and gilrs are
//! both polled, so instead of dispatching events we build one `InputFrame` per
//! rendered frame: `held` for movement, `pressed` for one-shot actions.

use erect_core::config::{GAMEPAD_DEADZONE, MAX_PLAYERS};
use erect_core::input::{InputFrame, MenuIntent, PlayerIntent};
use erect_core::settings::SchemeInfo;
use gilrs::{Axis, Button, Gilrs};
use macroquad::prelude::*;

/// What this platform offers. Index order is what gets stored in settings.
pub static SCHEMES: [SchemeInfo; 4] = [
    SchemeInfo { label: "W A S D + SPACE", is_gamepad: false, pad_index: 0 },
    SchemeInfo { label: "ARROWS + ENTER", is_gamepad: false, pad_index: 0 },
    SchemeInfo { label: "GAMEPAD 1", is_gamepad: true, pad_index: 0 },
    SchemeInfo { label: "GAMEPAD 2", is_gamepad: true, pad_index: 1 },
];

const SCHEME_KB_WASD: usize = 0;

/// Level state of one gamepad, so edges can be derived frame to frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PadState {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    jump: bool,
    attack: bool,
    back: bool,
    pause: bool,
    /// The shoulder button, kept apart from the face buttons: it is the only
    /// pad input the game asks for that is not already spoken for.
    dash: bool,
}

pub struct InputReader {
    gilrs: Option<Gilrs>,
    prev: [PadState; MAX_PLAYERS],
    cur: [PadState; MAX_PLAYERS],
    connected: [bool; MAX_PLAYERS],
}

impl InputReader {
    pub fn new() -> Self {
        // A missing gamepad subsystem must not stop the game from running.
        let gilrs = Gilrs::new().ok();
        Self {
            gilrs,
            prev: [PadState::default(); MAX_PLAYERS],
            cur: [PadState::default(); MAX_PLAYERS],
            connected: [false; MAX_PLAYERS],
        }
    }

    fn poll_pads(&mut self) {
        self.prev = self.cur;
        self.cur = [PadState::default(); MAX_PLAYERS];
        self.connected = [false; MAX_PLAYERS];

        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        // Drain the event queue so gilrs keeps its internal state current.
        while gilrs.next_event().is_some() {}

        for (slot, (_id, pad)) in gilrs.gamepads().enumerate() {
            if slot >= MAX_PLAYERS {
                break;
            }
            self.connected[slot] = true;
            let axis_x = pad.value(Axis::LeftStickX);
            let axis_y = pad.value(Axis::LeftStickY);
            self.cur[slot] = PadState {
                left: axis_x < -GAMEPAD_DEADZONE || pad.is_pressed(Button::DPadLeft),
                right: axis_x > GAMEPAD_DEADZONE || pad.is_pressed(Button::DPadRight),
                // gilrs reports stick up as positive Y, unlike the browser's axis.
                up: axis_y > GAMEPAD_DEADZONE || pad.is_pressed(Button::DPadUp),
                down: axis_y < -GAMEPAD_DEADZONE || pad.is_pressed(Button::DPadDown),
                jump: pad.is_pressed(Button::South),
                attack: pad.is_pressed(Button::West),
                back: pad.is_pressed(Button::East),
                pause: pad.is_pressed(Button::Start),
                dash: pad.is_pressed(Button::LeftTrigger)
                    || pad.is_pressed(Button::RightTrigger),
            };
        }
    }

    fn pad_pressed(&self, slot: usize, get: fn(&PadState) -> bool) -> bool {
        get(&self.cur[slot]) && !get(&self.prev[slot])
    }

    pub fn read(&mut self, player_schemes: &[usize]) -> InputFrame {
        self.poll_pads();

        let mut frame = InputFrame {
            pads_connected: self.connected,
            ..Default::default()
        };

        // Menus accept any keyboard and any pad, so a player can always navigate
        // regardless of what they picked in settings.
        frame.menu = MenuIntent {
            up: is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W),
            down: is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S),
            left: is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::A),
            right: is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::D),
            confirm: is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space),
            back: is_key_pressed(KeyCode::Escape),
        };
        frame.pause = is_key_pressed(KeyCode::Escape);
        // Nothing a player reaches for, and the core only acts on it from the
        // title screen.
        frame.dev_menu = is_key_pressed(KeyCode::Key8);

        for slot in 0..MAX_PLAYERS {
            if !self.connected[slot] {
                continue;
            }
            frame.menu.up |= self.pad_pressed(slot, |s| s.up);
            frame.menu.down |= self.pad_pressed(slot, |s| s.down);
            frame.menu.left |= self.pad_pressed(slot, |s| s.left);
            frame.menu.right |= self.pad_pressed(slot, |s| s.right);
            frame.menu.confirm |=
                self.pad_pressed(slot, |s| s.jump) || self.pad_pressed(slot, |s| s.attack);
            frame.menu.back |= self.pad_pressed(slot, |s| s.back);
            frame.pause |= self.pad_pressed(slot, |s| s.pause);
        }

        for (player_index, &scheme_index) in player_schemes.iter().enumerate().take(MAX_PLAYERS) {
            let scheme = &SCHEMES[scheme_index.min(SCHEMES.len() - 1)];
            frame.players[player_index] = match scheme.is_gamepad {
                false => {
                    if scheme_index == SCHEME_KB_WASD {
                        PlayerIntent {
                            left: is_key_down(KeyCode::A),
                            right: is_key_down(KeyCode::D),
                            jump: is_key_pressed(KeyCode::W),
                            slam: is_key_pressed(KeyCode::S),
                            attack: is_key_pressed(KeyCode::Space),
                            dash: is_key_pressed(KeyCode::LeftShift),
                        }
                    } else {
                        PlayerIntent {
                            left: is_key_down(KeyCode::Left),
                            right: is_key_down(KeyCode::Right),
                            jump: is_key_pressed(KeyCode::Up),
                            slam: is_key_pressed(KeyCode::Down),
                            attack: is_key_pressed(KeyCode::Enter),
                            // The second keyboard player gets the other Shift,
                            // which is the only key on that side of the board
                            // that pairs with the arrows the way LeftShift does
                            // with WASD.
                            dash: is_key_pressed(KeyCode::RightShift),
                        }
                    }
                }
                true => {
                    let slot = scheme.pad_index;
                    if slot >= MAX_PLAYERS || !self.connected[slot] {
                        PlayerIntent::default()
                    } else {
                        PlayerIntent {
                            left: self.cur[slot].left,
                            right: self.cur[slot].right,
                            jump: self.pad_pressed(slot, |s| s.jump),
                            attack: self.pad_pressed(slot, |s| s.attack),
                            // Slam is B or d-pad down, whichever the player reaches for.
                            slam: self.pad_pressed(slot, |s| s.back)
                                || self.pad_pressed(slot, |s| s.down),
                            dash: self.pad_pressed(slot, |s| s.dash),
                        }
                    }
                }
            };
        }

        frame
    }
}
