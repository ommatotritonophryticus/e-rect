//! The parallax skyline behind the field.
//!
//! Without it a scrolling camera over a flat background looks completely still:
//! nothing on screen would tell the player they are moving, only that enemies
//! are sliding about.
//!
//! The field is endless, so the blocks cannot be a stored list. Instead the
//! backdrop is divided into fixed-width slots and the block in slot `n` is
//! derived from a hash of `n`. That costs no memory, draws the same skyline
//! every time you walk back past it, and produces identical output on both
//! platforms.

use alloc::vec::Vec;

use crate::config::*;
use crate::entities::Rng;
use crate::geom::Viewport;

/// One block, already in screen coordinates and ready to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropBlock {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Blocks of one layer currently on screen, appended to `out` (cleared first).
///
/// `seed` ties the skyline to the run, so two runs do not look identical;
/// `parallax` is how much of the world's scroll this layer takes.
pub fn visible_blocks(
    camera_x: f32,
    v: &Viewport,
    seed: u64,
    parallax: f32,
    out: &mut Vec<BackdropBlock>,
) {
    out.clear();

    let slot = v.wper(BACKDROP_SLOT_PCT);
    if slot <= 0.0 {
        return;
    }

    // Each layer has its own, differently-paced world.
    let scroll = camera_x * parallax;
    let ground = v.hper(GROUND_Y_PCT);

    let first = libm::floorf(scroll / slot) as i64;
    let last = libm::floorf((scroll + v.w) / slot) as i64 + 1;

    for n in first..=last {
        // Multiplying by an odd constant scatters neighbouring slots, which a
        // plain `seed ^ n` would leave visibly similar.
        let mixed = seed ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut rng = Rng::new(mixed | 1);

        let w = lerp(v.wper(BACKDROP_MIN_W_PCT), v.wper(BACKDROP_MAX_W_PCT), rng.unit());
        let h = lerp(v.hper(BACKDROP_MIN_H_PCT), v.hper(BACKDROP_MAX_H_PCT), rng.unit());
        let jitter = (slot - w).max(0.0) * rng.unit();

        let world_x = n as f32 * slot + jitter;
        let x = world_x - scroll;
        if x > v.w || x + w < 0.0 {
            continue;
        }

        // Standing on the ground line. Tall blocks run up behind the ceiling
        // band, which is drawn afterwards and trims them.
        out.push(BackdropBlock {
            x,
            y: ground - h,
            w,
            h,
        });
    }
}
