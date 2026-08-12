//! sceGu drawing: flat rectangles and textured text quads.
//!
//! The game only ever draws axis-aligned rectangles, which is the GE's `Sprites`
//! primitive - two vertices per quad, no triangles needed.

use core::ffi::c_void;
use erect_core::color::Rgb;
use psp::sys::{
    self, BlendFactor, BlendOp, ClearBuffer, DisplayPixelFormat, GuContextType, GuPrimitive,
    GuState, GuSyncBehavior, GuSyncMode, MipmapLevel, TextureColorComponent, TextureEffect,
    TextureFilter, TexturePixelFormat, VertexType,
};
use psp::Align16;

use crate::font;

pub const SCREEN_W: i32 = 480;
pub const SCREEN_H: i32 = 272;
const BUF_W: i32 = 512;

static mut DISPLAY_LIST: Align16<[u32; 0x10000]> = Align16([0; 0x10000]);

/// Untextured vertex: colour then position. 4 + 6 bytes, padded to the 4-byte
/// stride the GE computes for this format.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VertexFlat {
    color: u32,
    x: i16,
    y: i16,
    z: i16,
    _pad: i16,
}

/// Textured vertex: UV, colour, position - in the order the GE expects.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VertexTex {
    u: i16,
    v: i16,
    color: u32,
    x: i16,
    y: i16,
    z: i16,
    _pad: i16,
}

/// PSP packs colours as 0xAABBGGRR.
pub fn pack(rgb: Rgb) -> u32 {
    let (r, g, b) = rgb.to_bytes();
    0xff00_0000 | ((b as u32) << 16) | ((g as u32) << 8) | r as u32
}

pub const WHITE: u32 = 0xffff_ffff;
pub const BLACK: u32 = 0xff00_0000;

/// # Safety
/// Call once at startup, before any drawing.
pub unsafe fn init() {
    unsafe {
        font::init();

        let list = core::ptr::addr_of_mut!(DISPLAY_LIST) as *mut c_void;
        sys::sceGuInit();
        sys::sceGuStart(GuContextType::Direct, list);
        sys::sceGuDrawBuffer(DisplayPixelFormat::Psm8888, core::ptr::null_mut(), BUF_W);
        sys::sceGuDispBuffer(SCREEN_W, SCREEN_H, 0x88000 as *mut c_void, BUF_W);
        sys::sceGuOffset(2048 - (SCREEN_W as u32 / 2), 2048 - (SCREEN_H as u32 / 2));
        sys::sceGuViewport(2048, 2048, SCREEN_W, SCREEN_H);
        sys::sceGuScissor(0, 0, SCREEN_W, SCREEN_H);
        sys::sceGuEnable(GuState::ScissorTest);

        // Only text needs blending - it is an alpha mask. Everything else in
        // this game is an opaque rectangle, and leaving blending on made every
        // one of them pay a read-modify-write of the framebuffer. The mode is
        // set up here and switched on only around glyphs.
        sys::sceGuBlendFunc(
            BlendOp::Add,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            0,
            0,
        );
        sys::sceGuDisable(GuState::Blend);

        sys::sceGuTexMode(TexturePixelFormat::Psm8888, 0, 0, 0);
        sys::sceGuTexFunc(TextureEffect::Modulate, TextureColorComponent::Rgba);
        sys::sceGuTexFilter(TextureFilter::Linear, TextureFilter::Linear);
        sys::sceGuTexScale(1.0, 1.0);
        sys::sceGuTexOffset(0.0, 0.0);

        sys::sceGuFinish();
        sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        sys::sceDisplayWaitVblankStart();
        sys::sceGuDisplay(true);
    }
}

/// # Safety
/// Must be paired with [`end_frame`].
pub unsafe fn begin_frame(clear: u32) {
    unsafe {
        let list = core::ptr::addr_of_mut!(DISPLAY_LIST) as *mut c_void;
        sys::sceGuStart(GuContextType::Direct, list);
        sys::sceGuClearColor(clear);
        sys::sceGuClear(ClearBuffer::COLOR_BUFFER_BIT);
    }
}

/// # Safety
/// Must follow [`begin_frame`].
pub unsafe fn end_frame() {
    unsafe {
        sys::sceGuFinish();
        sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        #[cfg(not(feature = "bench"))]
        sys::sceDisplayWaitVblankStart();
        sys::sceGuSwapBuffers();
    }
}

/// # Safety
/// Only valid between [`begin_frame`] and [`end_frame`].
pub unsafe fn rect(x: f32, y: f32, w: f32, h: f32, color: u32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    // Cheap reject: the game happily positions enemies far off-screen.
    if x > SCREEN_W as f32 || y > SCREEN_H as f32 || x + w < 0.0 || y + h < 0.0 {
        return;
    }

    unsafe {
        sys::sceGuDisable(GuState::Texture2D);
        let verts = sys::sceGuGetMemory(2 * core::mem::size_of::<VertexFlat>() as i32)
            as *mut VertexFlat;
        verts.write(VertexFlat {
            color,
            x: x as i16,
            y: y as i16,
            z: 0,
            _pad: 0,
        });
        verts.add(1).write(VertexFlat {
            color,
            x: (x + w) as i16,
            y: (y + h) as i16,
            z: 0,
            _pad: 0,
        });
        sys::sceGuDrawArray(
            GuPrimitive::Sprites,
            VertexType::COLOR_8888 | VertexType::VERTEX_16BIT | VertexType::TRANSFORM_2D,
            2,
            core::ptr::null(),
            verts as *const c_void,
        );
    }
}

/// Draws `text` with its baseline at `y`.
///
/// # Safety
/// Only valid between [`begin_frame`] and [`end_frame`].
pub unsafe fn text(x: f32, y: f32, size_px: f32, color: u32, text: &str) {
    unsafe {
        sys::sceGuEnable(GuState::Texture2D);
        sys::sceGuEnable(GuState::Blend);
        sys::sceGuTexImage(
            MipmapLevel::None,
            font::ATLAS_W as i32,
            font::ATLAS_H as i32,
            font::ATLAS_W as i32,
            font::atlas_ptr() as *const c_void,
        );

        let scale = font::scale_for(size_px);
        let mut pen = x;

        for ch in text.chars() {
            let Some(g) = font::glyph(ch) else { continue };
            if g.w > 0 && g.h > 0 {
                let gx = pen + g.left as f32 * scale;
                let gy = y + g.top as f32 * scale;
                let gw = g.w as f32 * scale;
                let gh = g.h as f32 * scale;

                if gx < SCREEN_W as f32 && gy < SCREEN_H as f32 && gx + gw > 0.0 && gy + gh > 0.0 {
                    let verts = sys::sceGuGetMemory(2 * core::mem::size_of::<VertexTex>() as i32)
                        as *mut VertexTex;
                    verts.write(VertexTex {
                        u: g.u as i16,
                        v: g.v as i16,
                        color,
                        x: gx as i16,
                        y: gy as i16,
                        z: 0,
                        _pad: 0,
                    });
                    verts.add(1).write(VertexTex {
                        u: (g.u + g.w) as i16,
                        v: (g.v + g.h) as i16,
                        color,
                        x: (gx + gw) as i16,
                        y: (gy + gh) as i16,
                        z: 0,
                        _pad: 0,
                    });
                    sys::sceGuDrawArray(
                        GuPrimitive::Sprites,
                        VertexType::TEXTURE_16BIT
                            | VertexType::COLOR_8888
                            | VertexType::VERTEX_16BIT
                            | VertexType::TRANSFORM_2D,
                        2,
                        core::ptr::null(),
                        verts as *const c_void,
                    );
                }
            }
            pen += g.advance * scale;
        }

        sys::sceGuDisable(GuState::Blend);
        sys::sceGuDisable(GuState::Texture2D);
    }
}

/// Text with the game's usual black drop shadow.
///
/// # Safety
/// Only valid between [`begin_frame`] and [`end_frame`].
pub unsafe fn text_shadowed(x: f32, y: f32, size_px: f32, color: u32, s: &str) {
    unsafe {
        text(x + 2.0, y + 2.0, size_px, BLACK, s);
        text(x, y, size_px, color, s);
    }
}

/// Shuts the graphics engine down before the game exits.
///
/// # Safety
/// Call once, after the last frame.
pub unsafe fn term() {
    unsafe {
        sys::sceGuDisplay(false);
        sys::sceGuTerm();
    }
}
