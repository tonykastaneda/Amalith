//! Fast ImageIO thumbnails. Thread-safe, and does not decode the full
//! raster when asking for a max pixel size — this is what makes Place
//! feel instant (256px) then sharpen (2048 / 8192).

use std::path::Path;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFMutableDictionary, CFNumber, CFRetained, CFString, CFType,
    CFURL,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextGetBytesPerRow, CGBitmapContextGetData,
    CGColorSpaceCreateDeviceRGB, CGContextDrawImage, CGContextScaleCTM, CGContextTranslateCTM,
    CGImage, CGImageAlphaInfo, CGImageGetHeight, CGImageGetWidth,
};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

#[repr(C)]
struct CGImageSource {
    _private: [u8; 0],
}

#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    fn CGImageSourceCreateWithURL(
        url: *const CFURL,
        options: *const CFDictionary,
    ) -> *mut CGImageSource;
    fn CGImageSourceCreateWithData(
        data: *const CFData,
        options: *const CFDictionary,
    ) -> *mut CGImageSource;
    fn CGImageSourceCreateThumbnailAtIndex(
        isrc: *mut CGImageSource,
        index: usize,
        options: *const CFDictionary,
    ) -> *mut CGImage;
    fn CGImageSourceCopyPropertiesAtIndex(
        isrc: *mut CGImageSource,
        index: usize,
        options: *const CFDictionary,
    ) -> *mut CFDictionary<CFString, CFType>;

    static kCGImageSourceThumbnailMaxPixelSize: &'static CFString;
    static kCGImageSourceCreateThumbnailFromImageAlways: &'static CFString;
    static kCGImageSourceCreateThumbnailWithTransform: &'static CFString;
    static kCGImagePropertyPixelWidth: &'static CFString;
    static kCGImagePropertyPixelHeight: &'static CFString;
}

fn as_cf_type<T>(v: &T) -> &CFType {
    unsafe { &*(v as *const T as *const CFType) }
}

fn thumb_options(max_side: u32) -> CFRetained<CFMutableDictionary<CFString, CFType>> {
    let dict = CFMutableDictionary::<CFString, CFType>::empty();
    let max = CFNumber::new_i32(max_side as i32);
    let yes = CFBoolean::new(true);
    unsafe {
        dict.set(kCGImageSourceThumbnailMaxPixelSize, as_cf_type(&*max));
        dict.set(kCGImageSourceCreateThumbnailFromImageAlways, as_cf_type(yes));
        dict.set(kCGImageSourceCreateThumbnailWithTransform, as_cf_type(yes));
    }
    dict
}

fn retain_src(ptr: *mut CGImageSource) -> Option<CFRetained<CFType>> {
    let p = NonNull::new(ptr)?;
    Some(unsafe { CFRetained::from_raw(p.cast::<CFType>()) })
}

fn src_ptr(src: &CFType) -> *mut CGImageSource {
    src as *const CFType as *mut CGImageSource
}

fn source_from_path(path: &Path) -> Option<CFRetained<CFType>> {
    let url = CFURL::from_file_path(path)?;
    retain_src(unsafe { CGImageSourceCreateWithURL(&*url, ptr::null()) })
}

fn source_from_bytes(bytes: &[u8]) -> Option<CFRetained<CFType>> {
    let data = CFData::from_bytes(bytes);
    retain_src(unsafe { CGImageSourceCreateWithData(&*data, ptr::null()) })
}

fn pixel_size_from_source(src: &CFType) -> Option<(u32, u32)> {
    let props = unsafe { CGImageSourceCopyPropertiesAtIndex(src_ptr(src), 0, ptr::null()) };
    let props = NonNull::new(props)?;
    let dict = unsafe { CFRetained::from_raw(props) };
    let w = dict
        .get(unsafe { kCGImagePropertyPixelWidth })
        .and_then(|v| v.downcast_ref::<CFNumber>().and_then(CFNumber::as_i32))?;
    let h = dict
        .get(unsafe { kCGImagePropertyPixelHeight })
        .and_then(|v| v.downcast_ref::<CFNumber>().and_then(CFNumber::as_i32))?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((w as u32, h as u32))
}

/// Native pixel size from ImageIO headers (no full decode).
pub fn pixel_size(path: &Path) -> Option<(u32, u32)> {
    let src = source_from_path(path)?;
    pixel_size_from_source(&src)
}

/// Native pixel size of encoded bytes.
pub fn pixel_size_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    let src = source_from_bytes(bytes)?;
    pixel_size_from_source(&src)
}

fn thumbnail_from_source(src: &CFType, max_side: u32) -> Option<ImageData> {
    let opts = thumb_options(max_side);
    let img = unsafe {
        CGImageSourceCreateThumbnailAtIndex(src_ptr(src), 0, &*opts as *const _ as *const CFDictionary)
    };
    let img = NonNull::new(img)?;
    let img = unsafe { CFRetained::<CGImage>::from_raw(img) };
    cgimage_to_gpu(&img)
}

pub fn thumbnail_path(path: &Path, max_side: u32) -> Option<ImageData> {
    let src = source_from_path(path)?;
    thumbnail_from_source(&src, max_side)
}

pub fn thumbnail_bytes(bytes: &[u8], max_side: u32) -> Option<ImageData> {
    let src = source_from_bytes(bytes)?;
    thumbnail_from_source(&src, max_side)
}

fn cgimage_to_gpu(image: &CGImage) -> Option<ImageData> {
    let w = CGImageGetWidth(Some(image));
    let h = CGImageGetHeight(Some(image));
    if w == 0 || h == 0 {
        return None;
    }
    let space = CGColorSpaceCreateDeviceRGB()?;
    let ctx = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            w,
            h,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    CGContextTranslateCTM(Some(&ctx), 0.0, h as f64);
    CGContextScaleCTM(Some(&ctx), 1.0, -1.0);
    let rect = objc2_core_foundation::CGRect {
        origin: objc2_core_foundation::CGPoint { x: 0.0, y: 0.0 },
        size: objc2_core_foundation::CGSize {
            width: w as f64,
            height: h as f64,
        },
    };
    CGContextDrawImage(Some(&ctx), rect, Some(image));
    let ptr = CGBitmapContextGetData(Some(&ctx)) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let stride = CGBitmapContextGetBytesPerRow(Some(&ctx));
    let row = w * 4;
    let mut rgba = vec![0u8; w * h * 4];
    let n = rgba.len();
    unsafe {
        if stride == row {
            rgba.copy_from_slice(std::slice::from_raw_parts(ptr, n));
        } else {
            for y in 0..h {
                let src = ptr.add(y * stride);
                let dst = y * row;
                rgba[dst..dst + row].copy_from_slice(std::slice::from_raw_parts(src, row));
            }
        }
    }
    Some(ImageData {
        data: Blob::from(rgba),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::AlphaPremultiplied,
        width: w as u32,
        height: h as u32,
    })
}
