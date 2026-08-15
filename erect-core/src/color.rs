//! RGB colour with the original's "ease toward a target" background behaviour.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn from_palette(index: usize) -> Self {
        let (_, r, g, b) = crate::config::PLAYER_COLORS[index];
        Self::new(r, g, b)
    }

    /// Channels as 0..=255 bytes. Frontends pack these however their API wants.
    pub fn to_bytes(self) -> (u8, u8, u8) {
        (
            self.r.clamp(0.0, 255.0) as u8,
            self.g.clamp(0.0, 255.0) as u8,
            self.b.clamp(0.0, 255.0) as u8,
        )
    }

    /// Darkened by an hp-style ratio; this is how damage is shown on every actor.
    pub fn shaded(self, ratio: f32) -> Rgb {
        let k = ratio.clamp(0.0, 1.0);
        Rgb::new(
            libm::floorf(self.r * k),
            libm::floorf(self.g * k),
            libm::floorf(self.b * k),
        )
    }
}

/// A colour that walks toward a target by a fixed step per tick. Used for the
/// background flashing red while a wave is running and green once it is cleared.
#[derive(Clone, Copy, Debug)]
pub struct EaseColor {
    pub cur: Rgb,
    pub target: Rgb,
}

impl EaseColor {
    pub fn new(cur: Rgb) -> Self {
        Self {
            cur,
            target: Rgb::new(0.0, 0.0, 0.0),
        }
    }

    pub fn set_target(&mut self, target: Rgb) {
        self.target = target;
    }

    pub fn approach(&mut self, step: f32) {
        Self::step_channel(&mut self.cur.r, self.target.r, step);
        Self::step_channel(&mut self.cur.g, self.target.g, step);
        Self::step_channel(&mut self.cur.b, self.target.b, step);
    }

    fn step_channel(value: &mut f32, target: f32, step: f32) {
        if (*value - target).abs() < step {
            *value = target;
            return;
        }
        if *value > target {
            *value -= step;
        } else if *value < target {
            *value += step;
        }
    }

    pub fn to_rgb(self) -> Rgb {
        self.cur
    }
}

