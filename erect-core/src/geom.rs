//! Screen-relative coordinates and axis-aligned collision.

/// Owns the current window size and converts the percentage-based coordinates
/// the whole game is written in into pixels.
///
/// The size is stored as plain floats and refreshed once per frame. The JS
/// version read `canvas.width`/`canvas.height` (DOM attributes) inside these
/// helpers, which cost hundreds of DOM crossings per frame.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub w: f32,
    pub h: f32,
}

impl Viewport {
    pub fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    pub fn sync(&mut self, w: f32, h: f32) {
        self.w = w;
        self.h = h;
    }

    #[inline]
    pub fn wper(&self, pct: f32) -> f32 {
        self.w / 100.0 * pct
    }

    #[inline]
    pub fn hper(&self, pct: f32) -> f32 {
        self.h / 100.0 * pct
    }
}

/// An axis-aligned box. Every collidable thing in the game is one of these.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Body {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Matches the original's inclusive overlap test exactly.
    #[inline]
    pub fn intersects(&self, other: &Body) -> bool {
        let x_coll = self.x + self.w >= other.x && self.x <= other.x + other.w;
        let y_coll = self.y + self.h >= other.y && self.y <= other.y + other.h;
        x_coll && y_coll
    }

    #[inline]
    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    #[inline]
    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }
}
