//! Framebuffer dumping, for checking the renderer from the host.
//!
//! Enabled by the `screenshot` feature only. The PSP has no way to hand a
//! picture back otherwise: PPSSPP's screenshot key cannot be driven from a
//! script, and macOS screen capture needs a permission the harness lacks.

use psp::sys::{self, DisplayPixelFormat, DisplaySetBufSync, IoOpenFlags};

pub const FB_STRIDE: usize = 512;
pub const FB_H: usize = 272;

/// Writes a short text file next to a dump, to say what the game was doing.
pub fn note(path: &[u8], text: &str) {
    unsafe {
        let fd = sys::sceIoOpen(
            path.as_ptr(),
            IoOpenFlags::CREAT | IoOpenFlags::TRUNC | IoOpenFlags::WR_ONLY,
            0o777,
        );
        if fd.0 >= 0 {
            sys::sceIoWrite(fd, text.as_ptr() as *const _, text.len());
            sys::sceIoClose(fd);
        }
    }
}

/// Writes the framebuffer currently on screen as raw 0xAABBGGRR words.
pub fn dump(path: &[u8]) {
    let mut topaddr: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut width: usize = 0;
    let mut format = DisplayPixelFormat::Psm8888;

    unsafe {
        sys::sceDisplayGetFrameBuf(
            &mut topaddr,
            &mut width,
            &mut format,
            DisplaySetBufSync::Immediate,
        );
        if topaddr.is_null() {
            return;
        }
        // VRAM read through the uncached mirror so the CPU sees what the GE wrote.
        let src = ((topaddr as usize) | 0x4000_0000) as *const u32;

        let fd = sys::sceIoOpen(
            path.as_ptr(),
            IoOpenFlags::CREAT | IoOpenFlags::TRUNC | IoOpenFlags::WR_ONLY,
            0o777,
        );
        if fd.0 < 0 {
            return;
        }
        // One row at a time: the buffer is 512 wide but only 480 is visible.
        for y in 0..FB_H {
            let row = src.add(y * FB_STRIDE);
            sys::sceIoWrite(fd, row as *const _, FB_STRIDE * 4);
        }
        sys::sceIoClose(fd);
    }
}
