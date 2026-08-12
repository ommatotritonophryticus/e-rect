//! Bitmap font baked by `build.rs`.
//!
//! The atlas ships as 8-bit coverage. At startup it is expanded to RGBA where
//! the colour is white and the alpha is the coverage, so a single texture works
//! for text of any colour: the vertex colour modulates it.

use psp::Align16;

#[derive(Clone, Copy)]
pub struct Glyph {
    pub u: u16,
    pub v: u16,
    pub w: u16,
    pub h: u16,
    /// Offset from the text baseline to the top of the glyph bitmap.
    pub top: i16,
    pub left: i16,
    pub advance: f32,
}

include!(concat!(env!("OUT_DIR"), "/font_table.rs"));

static ATLAS_ALPHA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font_atlas.bin"));

/// RGBA copy of the atlas, built once at startup. 512x256x4 = 512 KB, which is
/// affordable even on a 32 MB PSP-1000.
static mut ATLAS_RGBA: Align16<[u32; ATLAS_W * ATLAS_H]> = Align16([0; ATLAS_W * ATLAS_H]);

/// Expands the coverage atlas into the texture the GE samples.
///
/// # Safety
/// Must be called once, before any text is drawn.
pub unsafe fn init() {
    let dst = unsafe { &mut (*core::ptr::addr_of_mut!(ATLAS_RGBA)).0 };
    for (i, &coverage) in ATLAS_ALPHA.iter().enumerate() {
        // PSP stores 8888 as 0xAABBGGRR.
        dst[i] = ((coverage as u32) << 24) | 0x00ff_ffff;
    }
}

pub fn atlas_ptr() -> *const u32 {
    unsafe { (*core::ptr::addr_of!(ATLAS_RGBA)).0.as_ptr() }
}

pub fn glyph(ch: char) -> Option<&'static Glyph> {
    let code = ch as u32;
    if code < FIRST_CHAR as u32 || code > LAST_CHAR as u32 {
        return None;
    }
    GLYPHS.get((code - FIRST_CHAR as u32) as usize)
}

/// The fraction of the em taken up by capitals in the face the game's text
/// sizes were originally tuned against.
///
/// Every `size_px` in the renderer is a percentage of the viewport, chosen by
/// eye against that face. Treating the number as an em size makes it mean
/// something different in every font - swap in a face with smaller capitals
/// and the whole HUD quietly shrinks. Normalising by the baked cap height
/// instead keeps a given `size_px` the same height on screen whatever the
/// font, which is what the call sites assume.
const TUNED_CAP_RATIO: f32 = 0.8125;

/// Scale factor that renders text at `size_px` tall.
pub fn scale_for(size_px: f32) -> f32 {
    size_px * TUNED_CAP_RATIO / CAP_PX
}

pub fn text_width(text: &str, size_px: f32) -> f32 {
    let scale = scale_for(size_px);
    let mut width = 0.0;
    for ch in text.chars() {
        if let Some(g) = glyph(ch) {
            width += g.advance * scale;
        }
    }
    width
}
