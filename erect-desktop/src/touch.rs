//! The phone layout: a letterboxed play field with a gamepad drawn under it.
//!
//! The game's own coordinates are untouched. Everything it draws is a percentage
//! of `Viewport`, so confining it to a rectangle is a matter of handing it a
//! smaller viewport and pointing a camera at that rectangle - the renderer never
//! learns it is not filling the window.
//!
//! The pad is deliberately *below* the field rather than over it. A thumb on
//! glass covers about a centimetre, which on a phone-sized field is a quarter of
//! the play area; anything overlaid would hide the thing being played.

use erect_core::config::MAX_PLAYERS;
use erect_core::input::{InputFrame, MenuIntent, PlayerIntent};
use macroquad::prelude::*;

/// Fraction of the window height the play field is allowed.
///
/// The field's own bottom sixth is the black floor band, so the visible picture
/// ends higher than this - which is what leaves the pad its room.
const FIELD_H_FRAC: f32 = 0.60;

/// The shape the game is designed around: a body is 5% of the width by 10% of
/// the height, and only near this ratio does that come out square, the way it
/// does on a PSP.
const FIELD_ASPECT: f32 = 1.6;

/* ---------------- the pad, all of it ---------------- *
 *
 * Every number that decides how the controls look or where they answer lives
 * here. Two of them are load-bearing in a way that is easy to miss: the drawing
 * and the hit test read the *same* spread constants, so a button cannot end up
 * answering somewhere other than where it is painted. Change one of these and
 * both move together.
 *
 * They are all fractions, never pixels: the same build runs on a 1024-wide
 * window and a 3200-wide phone.
 */

/// Base size of everything on the pad, against the smaller window side.
/// A button is one of these across its radius, so raising it grows the lot.
const UNIT_FRAC: f32 = 0.080;

/// How much wider than drawn a control answers. A finger is not a cursor, and
/// it cannot see what it is covering.
const HIT_SLACK: f32 = 1.35;

/// Where the two clusters sit across the window, as a fraction of its width.
/// Move them toward 0.5 to bring them inward for smaller hands.
const DPAD_X_FRAC: f32 = 0.15;
const BUTTONS_X_FRAC: f32 = 0.85;

/// How far each d-pad arm sits from the centre, in units. Just over one, so the
/// four arms read as a cross rather than four islands.
const DPAD_ARM: f32 = 1.5;
/// Size of one arm, in units.
const DPAD_SEG: f32 = 1.2;
/// Half-width of the square the d-pad answers in, in units. Wider than the
/// cross it draws: a thumb slides off the arms and should keep steering.
const DPAD_ZONE: f32 = 2.4;
/// Nothing is pressed within this of the centre, in units, so a thumb resting
/// in the middle does not pick a direction it did not mean.
const DPAD_DEAD: f32 = 0.45;

/// How far each face button sits from the centre of the diamond, in units.
const BUTTON_SPREAD: f32 = 1.4;

/// The pause button, in units: its radius, and how far above the bottom edge.
const PAUSE_RADIUS: f32 = 0.6;
const PAUSE_BOTTOM_GAP: f32 = 1.2;

/// Grey the pad is drawn in.
fn pad_grey(alpha: f32) -> Color {
    Color::new(0.55, 0.55, 0.55, alpha)
}

/// Where everything sits, for one window size.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The play field, in window pixels.
    pub field: Rect,
    /// Centre of the d-pad.
    pub dpad: Vec2,
    /// Centre of the button diamond.
    pub buttons: Vec2,
    /// The pause button, in the empty middle the pad leaves.
    pub pause: Vec2,
    /// Button radius, and half a d-pad arm.
    pub unit: f32,
}

impl Layout {
    pub fn for_window(w: f32, h: f32) -> Self {
        // Widest field that fits both the window and the height allowance. On a
        // landscape screen the height binds; in portrait the width does.
        let field_w = (FIELD_ASPECT * FIELD_H_FRAC * h).min(w);
        let field_h = field_w / FIELD_ASPECT;
        let field = Rect::new(((w - field_w) / 2.0).floor(), 0.0, field_w, field_h);

        let unit = w.min(h) * UNIT_FRAC;
        // Centred in what is left under the field, but never so high that the
        // pad reaches into the picture.
        let cy = ((field.bottom() + h) / 2.0).max(field.bottom() + unit * 2.2);
        Self {
            field,
            dpad: vec2(w * DPAD_X_FRAC, cy),
            buttons: vec2(w * BUTTONS_X_FRAC, cy),
            pause: vec2(w * 0.5, h - unit * PAUSE_BOTTOM_GAP),
            unit,
        }
    }

    /// Camera that maps the game's `0..field.w, 0..field.h` onto the field.
    ///
    /// The viewport also clips, which matters: actors are thrown above the
    /// ceiling and popups drift past it, and without clipping they would spill
    /// out over the pad.
    pub fn camera(&self, window_h: f32) -> Camera2D {
        // Two different pixels meet here. `screen_width` and friends are in
        // logical pixels, which is what the game lays itself out in; a camera
        // viewport is in framebuffer pixels, and on any retina or phone screen
        // those differ by two or three. Mixing them puts the field at half size
        // in a corner.
        let dpi = macroquad::miniquad::window::dpi_scale();
        Camera2D {
            target: vec2(self.field.w / 2.0, self.field.h / 2.0),
            // Positive, so the game's y-down coordinates come out the right way
            // up: macroquad has already flipped the screen framebuffer, and
            // `from_display_rect` would flip it a second time.
            zoom: vec2(2.0 / self.field.w, 2.0 / self.field.h),
            // Measured from the bottom of the window, as GL counts.
            viewport: Some((
                (self.field.x * dpi) as i32,
                ((window_h - self.field.bottom()) * dpi) as i32,
                (self.field.w * dpi) as i32,
                (self.field.h * dpi) as i32,
            )),
            ..Default::default()
        }
    }
}

/// One frame of what the pad is being asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Held {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    /// A, bottom of the diamond.
    jump: bool,
    /// X, left.
    attack: bool,
    /// B, right.
    slam: bool,
    /// Y, top - the button no physical pad needed, same as Triangle on a PSP.
    dash: bool,
    pause: bool,
}

pub struct TouchPad {
    layout: Layout,
    previous: Held,
    /// Set once the player has actually touched the pad, so a desktop build
    /// showing the layout does not report a phantom press on the first frame.
    seen_input: bool,
}

impl TouchPad {
    pub fn new(w: f32, h: f32) -> Self {
        Self {
            layout: Layout::for_window(w, h),
            previous: Held::default(),
            seen_input: false,
        }
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn resize(&mut self, w: f32, h: f32) {
        self.layout = Layout::for_window(w, h);
    }

    /// Every point currently down: real touches, plus the mouse so the layout
    /// can be tried on a desktop without a touchscreen.
    fn points(&self) -> Vec<Vec2> {
        // Touch positions come back in framebuffer pixels while the layout is
        // in logical ones - the same two-units-of-pixel confusion the camera
        // viewport has, and on a phone the factor is two or three. Without this
        // every real touch lands off the pad, and the only thing that seemed to
        // work was the mouse macroquad simulates from the *last* touch, which
        // is why one finger looked fine and two did not.
        let dpi = macroquad::miniquad::window::dpi_scale().max(0.001);
        let mut out: Vec<Vec2> = touches()
            .iter()
            .filter(|t| t.phase != TouchPhase::Ended && t.phase != TouchPhase::Cancelled)
            .map(|t| t.position / dpi)
            .collect();
        // Both the level and the edge. A tap that opens and closes inside one
        // frame is never "down" when the frame is polled, and on a mouse that
        // is most clicks; the same is true of a quick stab at a phone screen.
        if is_mouse_button_down(MouseButton::Left) || is_mouse_button_pressed(MouseButton::Left) {
            let (x, y) = mouse_position();
            out.push(vec2(x, y));
        }
        out
    }

    fn read_held(&self) -> Held {
        let l = &self.layout;
        let mut h = Held::default();
        for p in self.points() {
            // The d-pad is a zone with a dead middle rather than four separate
            // pads: a thumb slides, and the gaps between four rects are where
            // it ends up.
            let d = p - l.dpad;
            let arm = l.unit * DPAD_ZONE;
            if d.x.abs() <= arm && d.y.abs() <= arm {
                let dead = l.unit * DPAD_DEAD;
                if d.x.abs() > d.y.abs() {
                    if d.x < -dead {
                        h.left = true;
                    } else if d.x > dead {
                        h.right = true;
                    }
                } else if d.y < -dead {
                    h.up = true;
                } else if d.y > dead {
                    h.down = true;
                }
            }

            let r = l.unit * HIT_SLACK;
            let spread = l.unit * BUTTON_SPREAD;
            if p.distance(l.buttons + vec2(0.0, spread)) <= r {
                h.jump = true;
            }
            if p.distance(l.buttons - vec2(spread, 0.0)) <= r {
                h.attack = true;
            }
            if p.distance(l.buttons + vec2(spread, 0.0)) <= r {
                h.slam = true;
            }
            if p.distance(l.buttons - vec2(0.0, spread)) <= r {
                h.dash = true;
            }
            if p.distance(l.pause) <= l.unit * PAUSE_RADIUS * HIT_SLACK {
                h.pause = true;
            }
        }
        h
    }

    /// Folds the pad into an input frame. Player two is not served: two d-pads
    /// and eight buttons do not fit on one phone.
    pub fn read(&mut self) -> InputFrame {
        let now = self.read_held();
        if now != Held::default() {
            self.seen_input = true;
        }
        let was = self.previous;
        self.previous = now;
        let edge = |cur: bool, prev: bool| cur && !prev;

        let mut frame = InputFrame {
            pads_connected: [false; MAX_PLAYERS],
            ..Default::default()
        };
        frame.players[0] = PlayerIntent {
            left: now.left,
            right: now.right,
            jump: edge(now.jump, was.jump),
            slam: edge(now.slam, was.slam) || edge(now.down, was.down),
            attack: edge(now.attack, was.attack),
            attack_held: now.attack,
            dash: edge(now.dash, was.dash),
        };
        frame.menu = MenuIntent {
            up: edge(now.up, was.up),
            down: edge(now.down, was.down),
            left: edge(now.left, was.left),
            right: edge(now.right, was.right),
            confirm: edge(now.jump, was.jump) || edge(now.attack, was.attack),
            back: edge(now.slam, was.slam),
        };
        frame.pause = edge(now.pause, was.pause);
        frame
    }

    /// Draws the pad. Pressed controls brighten, which is the only feedback a
    /// finger gets - it is covering the thing it just pressed.
    pub fn draw(&self, font: &Font) {
        let l = &self.layout;
        let held = self.previous;
        let u = l.unit;
        let seg = u * DPAD_SEG;

        // D-pad: four arms around an empty middle.
        let arms = [
            (vec2(-u * DPAD_ARM, 0.0), held.left),
            (vec2(u * DPAD_ARM, 0.0), held.right),
            (vec2(0.0, -u * DPAD_ARM), held.up),
            (vec2(0.0, u * DPAD_ARM), held.down),
        ];
        for (offset, on) in arms {
            let c = l.dpad + offset;
            draw_rectangle(
                c.x - seg / 2.0,
                c.y - seg / 2.0,
                seg,
                seg,
                pad_grey(if on { 0.95 } else { 0.55 }),
            );
        }

        // Face buttons, in the layout every pad has used for thirty years.
        let face = [
            (vec2(0.0, u * BUTTON_SPREAD), "A", held.jump),
            (vec2(-u * BUTTON_SPREAD, 0.0), "X", held.attack),
            (vec2(u * BUTTON_SPREAD, 0.0), "B", held.slam),
            (vec2(0.0, -u * BUTTON_SPREAD), "Y", held.dash),
        ];
        for (offset, label, on) in face {
            let c = l.buttons + offset;
            draw_circle(c.x, c.y, u, pad_grey(if on { 0.95 } else { 0.55 }));
            let size = (u * 1.1) as u16;
            let m = measure_text(label, Some(font), size, 1.0);
            draw_text_ex(
                label,
                c.x - m.width / 2.0,
                c.y + m.height / 2.0,
                TextParams {
                    font: Some(font),
                    font_size: size,
                    color: Color::new(0.1, 0.1, 0.1, 1.0),
                    ..Default::default()
                },
            );
        }

        // Pause, in the gap the two thumbs leave between them. The mockup had
        // nowhere for it, and without one a phone cannot reach the menu at all.
        draw_circle(
            l.pause.x,
            l.pause.y,
            u * PAUSE_RADIUS,
            pad_grey(if held.pause { 0.95 } else { 0.4 }),
        );
        let bar = u * 0.1;
        for side in [-1.0f32, 1.0] {
            draw_rectangle(
                l.pause.x + side * u * 0.18 - bar / 2.0,
                l.pause.y - u * 0.22,
                bar,
                u * 0.44,
                Color::new(0.1, 0.1, 0.1, 1.0),
            );
        }
    }
}
