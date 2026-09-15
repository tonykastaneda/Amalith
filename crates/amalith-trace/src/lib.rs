//! Native tracing adapter. Coordinates are returned in original image pixels.
use amalith_core::{Color, PathData};
use kurbo::{Affine, BezPath};
use serde::{Deserialize, Serialize};
pub use vtracer::progress::CancelToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    BlackWhite,
    Grayscale,
    Color,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub mode: Mode,
    pub palette_mode: u8,
    pub tone_detail: u8,
    pub palette: Vec<[u8; 3]>,
    pub threshold: u8,
    pub colors: u16,
    pub paths: f64,
    pub corners: f64,
    pub noise: u16,
    pub transparency: bool,
    pub ignore: bool,
    pub ignore_color: [u8; 3],
}
impl Default for Options {
    fn default() -> Self {
        Self {
            palette_mode: 0,
            tone_detail: 50,
            palette: Vec::new(),
            mode: Mode::BlackWhite,
            threshold: 128,
            colors: 16,
            paths: 50.,
            corners: 75.,
            noise: 25,
            transparency: true,
            ignore: false,
            ignore_color: [255; 3],
        }
    }
}
impl Options {
    pub fn normalize(&mut self) {
        self.colors = self.colors.clamp(2, 256);
        self.palette_mode = self.palette_mode.min(2);
        self.tone_detail = self.tone_detail.min(100);
        self.paths = if self.paths.is_finite() {
            self.paths.clamp(0., 100.)
        } else {
            50.
        };
        self.corners = if self.corners.is_finite() {
            self.corners.clamp(0., 100.)
        } else {
            75.
        };
        self.noise = self.noise.clamp(1, 100);
    }
    pub fn presets() -> Vec<(&'static str, Self)> {
        vec![
            ("Default", Self::default()),
            (
                "Black and White Logo",
                Self {
                    paths: 85.,
                    noise: 2,
                    ignore: true,
                    ..Self::default()
                },
            ),
            (
                "Line Art",
                Self {
                    paths: 90.,
                    corners: 90.,
                    noise: 1,
                    ignore: true,
                    ..Self::default()
                },
            ),
            (
                "6 Colors",
                Self {
                    mode: Mode::Color,
                    colors: 6,
                    ..Self::default()
                },
            ),
            (
                "16 Colors",
                Self {
                    mode: Mode::Color,
                    ..Self::default()
                },
            ),
            (
                "Grayscale",
                Self {
                    mode: Mode::Grayscale,
                    ..Self::default()
                },
            ),
            (
                "Photo",
                Self {
                    mode: Mode::Color,
                    colors: 64,
                    paths: 70.,
                    noise: 10,
                    ..Self::default()
                },
            ),
        ]
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ResultPaths {
    pub paths: Vec<(PathData, Color)>,
    pub width: u32,
    pub height: u32,
    pub anchors: usize,
    pub colors: usize,
    pub reduced: bool,
}
/// A decoded source and reusable segmentation for successive slider changes.
pub struct Tracer {
    source: image::RgbaImage,
    prepared: Option<(Options, bool, vtracer::Session)>,
}
impl Tracer {
    pub fn sample(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.source.width() || y >= self.source.height() {
            return None;
        }
        let p = self.source.get_pixel(x, y);
        Some([p[0], p[1], p[2]])
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.source.dimensions()
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || w as u64 * h as u64 > 32_000_000 {
            return Err("Image Trace supports images up to 32 megapixels.".into());
        }
        let source = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        Ok(Self {
            source,
            prepared: None,
        })
    }
    pub fn trace(
        &mut self,
        options: &Options,
        full: bool,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(f64),
    ) -> Result<ResultPaths, String> {
        if cancel.is_cancelled() {
            return Err("Trace cancelled".into());
        }
        let mut opts = options.clone();
        opts.normalize();
        let (ow, oh) = self.source.dimensions();
        let reduced = !full && ow.max(oh) > 1024;
        // Only preprocessing settings invalidate the source/session. Curve and color
        // quantization changes reuse VTracer's segmentation where possible.
        let rebuild = self.prepared.as_ref().is_none_or(|(old, old_full, _)| {
            *old_full != full
                || old.mode != opts.mode
                || old.threshold != opts.threshold
                || old.transparency != opts.transparency
                || old.ignore != opts.ignore
                || old.ignore_color != opts.ignore_color
        });
        if rebuild {
            let img = if reduced {
                image::DynamicImage::ImageRgba8(self.source.clone())
                    .resize(1024, 1024, image::imageops::FilterType::Triangle)
                    .to_rgba8()
            } else {
                self.source.clone()
            };
            let (w, h) = img.dimensions();
            // A transparent border guarantees VTracer keys alpha even when the
            // original image has only a tiny transparent hole. Remove it in the
            // coordinate conversion below. Ignore Color is masked before fitting.
            let mut pixels = vec![0u8; ((w + 2) * (h + 2) * 4) as usize];
            for (x, y, p) in img.enumerate_pixels() {
                if x == 0 && y % 64 == 0 && cancel.is_cancelled() {
                    return Err("Trace cancelled".into());
                }
                let [r, g, b, a] = p.0;
                if (opts.transparency && a < 128)
                    || (opts.ignore
                        && [r, g, b]
                            .iter()
                            .zip(opts.ignore_color)
                            .all(|(a, b)| a.abs_diff(b) <= 8))
                {
                    continue;
                }
                let mut rgb = [r, g, b];
                for c in &mut rgb {
                    *c = ((*c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
                }
                let gray =
                    (0.2126 * rgb[0] as f64 + 0.7152 * rgb[1] as f64 + 0.0722 * rgb[2] as f64)
                        .round() as u8;
                if opts.mode == Mode::Grayscale {
                    rgb = [gray; 3];
                }
                if opts.mode == Mode::BlackWhite {
                    rgb = [if gray < opts.threshold { 0 } else { 255 }; 3];
                }
                let i = (((y + 1) * (w + 2) + x + 1) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            self.prepared = Some((
                opts.clone(),
                full,
                vtracer::Session::new(vtracer::ColorImage {
                    pixels,
                    width: (w + 2) as usize,
                    height: (h + 2) as usize,
                }),
            ));
        }
        let session = &mut self.prepared.as_mut().unwrap().2;
        if session.image().pixels.chunks_exact(4).all(|p| p[3] == 0) {
            return Ok(ResultPaths {
                paths: Vec::new(),
                width: ow,
                height: oh,
                anchors: 0,
                colors: 0,
                reduced,
            });
        }
        let w = session.image().width as u32 - 2;
        let h = session.image().height as u32 - 2;
        let ratio = w as f64 / ow as f64;
        let mut cfg = vtracer::Config::default();
        cfg.hierarchical = vtracer::Hierarchical::Cutout;
        cfg.color_precision = if opts.mode == Mode::Color && opts.palette_mode == 1 {
            1 + (opts.tone_detail as i32 * 7 / 100)
        } else {
            8
        };
        cfg.layer_difference = if opts.mode == Mode::BlackWhite { 0 } else { 16 };
        if opts.mode == Mode::Color && opts.palette_mode == 1 {
            cfg.layer_difference = 64 - opts.tone_detail as i32 * 56 / 100;
        }
        cfg.max_colors = if opts.mode == Mode::Color && opts.palette_mode == 1 {
            None
        } else {
            Some(if opts.mode == Mode::BlackWhite {
                2
            } else {
                opts.colors as usize
            })
        };
        if opts.mode == Mode::Color && opts.palette_mode == 2 {
            if opts.palette.is_empty() {
                return Err("Add solid colors to Document Swatches first.".into());
            }
            cfg.palette = opts
                .palette
                .iter()
                .map(|c| vtracer::Color::new(c[0], c[1], c[2]))
                .collect();
        }
        cfg.filter_speckle = ((opts.noise as f64 * ratio * ratio).sqrt().round() as usize).max(1);
        cfg.corner_threshold = (120. - opts.corners * 1.1).round() as i32;
        cfg.length_threshold = 3.5 + (100. - opts.paths) * 0.065;
        cfg.simplify = Some((0.15 + (100. - opts.paths) * 0.025) * ratio);
        cfg.path_precision = Some(4);
        let doc = session
            .render_with_progress(&cfg, cancel, &mut |p| progress(p.fraction as f64))
            .map_err(|e| e.to_string())?;
        let xf = Affine::scale_non_uniform(ow as f64 / w as f64, oh as f64 / h as f64)
            * Affine::translate((-1., -1.));
        let mut paths = Vec::new();
        let mut anchors = 0;
        let mut colors = std::collections::HashSet::new();
        for shape in doc.shapes {
            if cancel.is_cancelled() {
                return Err("Trace cancelled".into());
            }
            let c = shape.paint.color();
            colors.insert((c.r, c.g, c.b));
            let mut path = BezPath::new();
            for sub in shape.path.subpaths {
                for cmd in sub.commands {
                    use vtracer::ir::PathCmd::*;
                    match cmd {
                        MoveTo(p) => {
                            path.move_to((p.x, p.y));
                            anchors += 1;
                        }
                        LineTo(p) => {
                            path.line_to((p.x, p.y));
                            anchors += 1;
                        }
                        CubicTo(a, b, c) => {
                            path.curve_to((a.x, a.y), (b.x, b.y), (c.x, c.y));
                            anchors += 1;
                        }
                        Close => path.close_path(),
                    }
                }
            }
            if !path.is_empty() {
                paths.push((
                    PathData::from_bezpath(xf * path),
                    Color::rgb(c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.),
                ));
            }
            if paths.len() > 100_000 || anchors > 1_000_000 {
                return Err(
                    "Trace is too complex. Reduce Paths or Colors, or increase Noise.".into(),
                );
            }
        }
        Ok(ResultPaths {
            paths,
            width: ow,
            height: oh,
            anchors,
            colors: colors.len(),
            reduced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;
    fn tracer(img: image::RgbaImage) -> Tracer {
        Tracer {
            source: img,
            prepared: None,
        }
    }
    fn color_at(r: &ResultPaths, x: f64, y: f64) -> Option<Color> {
        r.paths
            .iter()
            .rev()
            .find(|(p, _)| p.geometry.winding((x, y).into()) != 0)
            .map(|(_, c)| *c)
    }
    #[test]
    fn threshold_traces_both_black_and_white() {
        let img = image::RgbaImage::from_fn(32, 16, |x, _| {
            image::Rgba(if x < 16 {
                [80, 80, 80, 255]
            } else {
                [200, 200, 200, 255]
            })
        });
        let mut t = tracer(img);
        let o = Options {
            noise: 1,
            paths: 100.,
            ..Default::default()
        };
        let r = t.trace(&o, true, &CancelToken::new(), &mut |_| {}).unwrap();
        assert_eq!(color_at(&r, 8., 8.).unwrap(), Color::rgb(0., 0., 0.));
        assert_eq!(color_at(&r, 24., 8.).unwrap(), Color::rgb(1., 1., 1.));
    }
    #[test]
    fn tiny_transparent_hole_is_preserved_and_ignore_color_is_masked() {
        let img = image::RgbaImage::from_fn(40, 40, |x, y| {
            image::Rgba(if (18..22).contains(&x) && (18..22).contains(&y) {
                [0, 0, 0, 0]
            } else {
                [255, 0, 0, 255]
            })
        });
        let mut t = tracer(img);
        let o = Options {
            mode: Mode::Color,
            noise: 1,
            paths: 100.,
            ..Default::default()
        };
        let r = t.trace(&o, true, &CancelToken::new(), &mut |_| {}).unwrap();
        assert!(color_at(&r, 20., 20.).is_none());
        assert!(color_at(&r, 8., 8.).unwrap().r > 0.9);
        let o = Options {
            ignore: true,
            ignore_color: [255, 0, 0],
            ..o
        };
        let r = t.trace(&o, true, &CancelToken::new(), &mut |_| {}).unwrap();
        assert!(r.paths.is_empty());
    }
    #[test]
    fn grayscale_palette_cancel_and_preview_coordinates() {
        let mut t = tracer(image::RgbaImage::from_pixel(
            1200,
            60,
            image::Rgba([70, 120, 180, 255]),
        ));
        let o = Options {
            mode: Mode::Grayscale,
            noise: 1,
            ..Default::default()
        };
        let r = t
            .trace(&o, false, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert!(r.reduced);
        assert_eq!((r.width, r.height), (1200, 60));
        assert!(color_at(&r, 1100., 30.).is_some());
        for (_, c) in r.paths {
            assert_eq!(c.r, c.g);
            assert_eq!(c.g, c.b);
        }
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(t.trace(&o, true, &cancel, &mut |_| {}).is_err());
    }
}
