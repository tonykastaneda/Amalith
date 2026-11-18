//! The one thing GPUI 0.2.2 can't do: move a window from code. A live
//! tear-off needs its new window to follow the cursor, so on macOS this
//! reaches through the raw window handle to the `NSWindow` itself.
//! Elsewhere it's a no-op and a torn-off window stays where it opened.

use gpui::Window;

#[cfg(target_os = "macos")]
pub type NativeWindow = objc2::rc::Retained<objc2_app_kit::NSWindow>;
#[cfg(not(target_os = "macos"))]
pub type NativeWindow = ();

#[cfg(target_os = "macos")]
pub fn native_window(window: &Window) -> Option<NativeWindow> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else { return None };
    // SAFETY: GPUI's AppKit handle points at the window's live NSView.
    let view: &objc2_app_kit::NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

#[cfg(not(target_os = "macos"))]
pub fn native_window(_: &Window) -> Option<NativeWindow> {
    None
}

/// Puts the window's top-left frame corner at (`x`, `y`): top-left-origin
/// logical px on the primary display, the same space GPUI reports bounds in.
#[cfg(target_os = "macos")]
pub fn move_window(window: &NativeWindow, x: f32, y: f32) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let Some(primary) = objc2_app_kit::NSScreen::screens(mtm).firstObject() else { return };
    let height = primary.frame().size.height;
    let point = objc2_foundation::NSPoint::new(x as f64, height - y as f64);
    #[allow(unused_unsafe)]
    unsafe {
        window.setFrameTopLeftPoint(point);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn move_window(_: &NativeWindow, _: f32, _: f32) {}
