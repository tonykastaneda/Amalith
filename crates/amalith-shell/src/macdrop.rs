//! macOS drag-and-drop beyond winit's `NSFilenamesPboardType` handler.
//!
//! winit only registers legacy file-path drags. iMessage (and Mail, Photos,
//! Safari) typically put a *file promise* and/or TIFF/PNG pasteboard image
//! on the drag, not a Finder path — so a photo dragged out of Messages
//! never reaches [`winit::event::WindowEvent::DroppedFile`].
//!
//! A transparent overlay view sits on the content view, accepts those extra
//! pasteboard types, and queues files/bytes for the app to Place. Mouse
//! events are forwarded to winit's view underneath. ImageIO via `NSImage`
//! also covers HEIC, which Messages attachments often are.

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBitmapImageFileType, NSBitmapImageRep, NSDragOperation, NSEvent,
    NSFilePromiseReceiver, NSImage, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG,
    NSPasteboardTypeTIFF, NSView,
};
use objc2_foundation::{NSArray, NSDictionary, NSObject, NSString, NSURL};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// A dropped raster, ready to Place.
pub enum Incoming {
    Path(PathBuf),
    /// PNG bytes (from a pasteboard image that was never a file).
    Png { name: String, bytes: Vec<u8> },
}

pub struct Dropped {
    pub item: Incoming,
    /// Window-view coordinates, top-left origin, logical px. `None` if unknown.
    pub at: Option<(f64, f64)>,
}

static QUEUE: Mutex<Vec<Dropped>> = Mutex::new(Vec::new());

pub fn drain() -> Vec<Dropped> {
    QUEUE.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

fn push(item: Incoming, at: Option<(f64, f64)>) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(Dropped { item, at });
    }
}



struct DropIvars;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DropIvars]
    #[name = "AmalithDropCatcher"]
    struct DropCatcher;

    impl DropCatcher {
        /// Match winit's view so drop coordinates are top-left, not flipped.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            fwd(self, sel!(mouseDown:), event);
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            fwd(self, sel!(mouseUp:), event);
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            fwd(self, sel!(rightMouseDown:), event);
        }
        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            fwd(self, sel!(rightMouseUp:), event);
        }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            fwd(self, sel!(otherMouseDown:), event);
        }
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            fwd(self, sel!(otherMouseUp:), event);
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            fwd(self, sel!(mouseMoved:), event);
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            fwd(self, sel!(mouseDragged:), event);
        }
        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            fwd(self, sel!(rightMouseDragged:), event);
        }
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            fwd(self, sel!(otherMouseDragged:), event);
        }
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            fwd(self, sel!(mouseEntered:), event);
        }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: &NSEvent) {
            fwd(self, sel!(mouseExited:), event);
        }
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            fwd(self, sel!(scrollWheel:), event);
        }
        #[unsafe(method(magnifyWithEvent:))]
        fn magnify(&self, event: &NSEvent) {
            fwd(self, sel!(magnifyWithEvent:), event);
        }
        #[unsafe(method(smartMagnifyWithEvent:))]
        fn smart_magnify(&self, event: &NSEvent) {
            fwd(self, sel!(smartMagnifyWithEvent:), event);
        }
        #[unsafe(method(rotateWithEvent:))]
        fn rotate(&self, event: &NSEvent) {
            fwd(self, sel!(rotateWithEvent:), event);
        }
        #[unsafe(method(pressureChangeWithEvent:))]
        fn pressure(&self, event: &NSEvent) {
            fwd(self, sel!(pressureChangeWithEvent:), event);
        }

        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, _sender: &NSObject) -> usize {
            NSDragOperation::Copy.0
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, _sender: &NSObject) -> usize {
            NSDragOperation::Copy.0
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag(&self, _sender: &NSObject) -> bool {
            true
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &NSObject) -> bool {
            let at = drop_point(self, sender);
            let pb: Retained<NSPasteboard> = unsafe { msg_send![sender, draggingPasteboard] };
            // Prefer an existing file path and link it in place. Never ask
            // the source to write a promised copy into a temp folder.
            take_filenames(&pb, at)
                || take_file_url(&pb, at)
                || take_pasteboard_image(&pb, at)
        }
    }
);

fn fwd(this: &DropCatcher, sel: objc2::runtime::Sel, event: &NSEvent) {
    if let Some(sv) = unsafe { this.superview() } {
        // `performSelector:withObject:` is declared to return `id`.
        let _: *mut objc2::runtime::AnyObject =
            unsafe { msg_send![&*sv, performSelector: sel, withObject: event] };
    }
}

/// Swap the main window's content view to our dragging-destination subclass
/// and register iMessage-friendly pasteboard types.
pub fn install(window: &Window) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    let ns_view = appkit.ns_view.as_ptr() as *mut NSView;
    if ns_view.is_null() {
        return;
    }
    let parent = unsafe { &*ns_view };
    for i in 0..parent.subviews().count() {
        let sv = parent.subviews().objectAtIndex(i);
        if sv.class().name() == c"AmalithDropCatcher" {
            return;
        }
    }
    let mtm = objc2::MainThreadMarker::from(parent);
    let this = DropCatcher::alloc(mtm).set_ivars(DropIvars);
    let catcher: Retained<DropCatcher> =
        unsafe { msg_send![super(this), initWithFrame: parent.bounds()] };
    catcher.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    register_types(&catcher);
    parent.addSubview(&catcher);
}

fn register_types(view: &NSView) {
    let mut types: Vec<Retained<NSString>> = Vec::new();
    types.push(unsafe { NSPasteboardTypeFileURL }.retain());
    types.push(unsafe { NSPasteboardTypeTIFF }.retain());
    types.push(unsafe { NSPasteboardTypePNG }.retain());
    #[allow(deprecated)]
    {
        types.push(unsafe { objc2_app_kit::NSFilenamesPboardType }.retain());
        types.push(unsafe { objc2_app_kit::NSFilesPromisePboardType }.retain());
    }
    let readable = NSFilePromiseReceiver::readableDraggedTypes();
    for i in 0..readable.count() {
        types.push(readable.objectAtIndex(i));
    }
    view.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
}

fn drop_point(this: &NSView, sender: &NSObject) -> Option<(f64, f64)> {
    let loc: objc2_foundation::NSPoint = unsafe { msg_send![sender, draggingLocation] };
    let converted: objc2_foundation::NSPoint =
        unsafe { msg_send![this, convertPoint: loc, fromView: Option::<&NSView>::None] };
    Some((converted.x, converted.y))
}

fn take_filenames(pb: &NSPasteboard, at: Option<(f64, f64)>) -> bool {
    #[allow(deprecated)]
    let Some(list) = pb.propertyListForType(unsafe { objc2_app_kit::NSFilenamesPboardType }) else {
        return false;
    };
    let Ok(names) = list.downcast::<NSArray>() else {
        return false;
    };
    let mut any = false;
    for i in 0..names.count() {
        let obj = names.objectAtIndex(i);
        let Ok(s) = obj.downcast::<NSString>() else {
            continue;
        };
        let path = PathBuf::from(s.to_string());
        if path.is_file() {
            push(Incoming::Path(path), at);
            any = true;
        }
    }
    any
}

fn take_file_url(pb: &NSPasteboard, at: Option<(f64, f64)>) -> bool {
    let Some(s) = pb.stringForType(unsafe { NSPasteboardTypeFileURL }) else {
        return false;
    };
    let Some(url) = NSURL::URLWithString(&s) else {
        return false;
    };
    let Some(path) = url.path() else {
        return false;
    };
    let path = PathBuf::from(path.to_string());
    if path.is_file() {
        push(Incoming::Path(path), at);
        true
    } else {
        false
    }
}

fn nsimage_png_bytes(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(png.to_vec())
}

fn take_pasteboard_image(pb: &NSPasteboard, at: Option<(f64, f64)>) -> bool {
    let Some(image) = NSImage::initWithPasteboard(NSImage::alloc(), pb) else {
        return false;
    };
    let Some(bytes) = nsimage_png_bytes(&image) else {
        return false;
    };
    push(
        Incoming::Png {
            name: "Image".into(),
            bytes,
        },
        at,
    );
    true
}
