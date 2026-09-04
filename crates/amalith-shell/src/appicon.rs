//! App icon wiring.
//!
//! The shell runs as a bare binary — there's no `.app` bundle yet to carry an
//! icon — so we set it at runtime instead:
//!
//! - macOS: `NSApplication.applicationIconImage`, which drives the Dock and the
//!   Cmd-Tab switcher. winit's `with_window_icon` is a documented no-op here.
//! - Windows / Linux: [`window_icon`] feeds `WindowAttributes::with_window_icon`
//!   for the title bar and taskbar.
//!
//! Both read the same art: `assets/app-icon.png`, embedded at build time.
//! Replace that file with the real export (a square PNG, 1024×1024) and both
//! paths pick it up on the next build.

/// The icon art, embedded at build time.
const ICON_PNG: &[u8] = include_bytes!("../assets/app-icon.png");

/// Decode a PNG byte slice to `(rgba8, width, height)`. Shared with the About
/// splash ([`crate::about`]).
pub fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        // Anything else (palette, grayscale) means the export isn't what we
        // asked for — skip rather than guess.
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// A winit window icon for the platforms that honour one. Always `None` on
/// macOS, where the Dock icon is set through [`set_dock_icon`] instead.
pub fn window_icon() -> Option<winit::window::Icon> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let (rgba, w, h) = decode_png(ICON_PNG)?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// Set the macOS Dock / Cmd-Tab icon. A no-op on every other platform.
pub fn set_dock_icon() {
    #[cfg(target_os = "macos")]
    {
        use objc2::{AnyThread, MainThreadMarker};
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::NSData;

        // AppKit calls must be on the main thread; this runs from `resumed`,
        // which already is, but check rather than assume.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // NSImage wants an encoded container, not raw pixels — hand it the PNG.
        let data = NSData::with_bytes(ICON_PNG);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}
