//! Sparse stroke coverage and immutable GPU preview tiles. Full image
//! assembly happens only at commit, keeping the existing asset/undo format.
use crate::lod::{ImageLods, RasterTile, RasterTiles};
use image::{Pixel, Rgba, RgbaImage};
use std::collections::{HashMap, HashSet};

pub(super) const TILE: u32 = 256;
static CLEAR: Rgba<u8> = Rgba([0; 4]);

pub(super) struct PaintTiles {
    width: u32,
    height: u32,
    ink: HashMap<(u32, u32), RgbaImage>,
    dirty: HashSet<(u32, u32)>,
    gpu: Vec<RasterTile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GPU; compares tiled and whole-image rendering"]
    fn gpu_tiles_match_whole_image_under_rotation_and_transparency() {
        use vello::{
            kurbo::{Affine, Rect},
            peniko::{Blob, Color, ImageAlphaType, ImageData, ImageFormat},
            wgpu,
        };
        let mut context = vello::util::RenderContext::new();
        let index = pollster::block_on(context.device(None)).expect("GPU adapter");
        let dev = &context.devices[index];
        let mut renderer = vello::Renderer::new(&dev.device, Default::default()).unwrap();
        let mut doc = amalith_core::Document::new("tile parity");
        let layer = amalith_core::LayerId::new();
        let object = amalith_core::ObjectId::new();
        let asset = amalith_core::AssetId::new();
        doc.insert_layer(amalith_core::Layer::new(layer, "image"), 0);
        let mut obj = amalith_core::Object::new(
            object,
            amalith_core::ObjectParent::Layer(layer),
            amalith_core::ObjectKind::Image(amalith_core::ImageData {
                asset,
                local_bounds: amalith_core::Rect::new(0., 0., 513., 257.),
                mask: None,
            }),
        );
        obj.transform = crate::convert::affine_to_core(
            Affine::translate((100.3, 100.7)) * Affine::rotate(0.19) * Affine::scale(0.65),
        );
        doc.insert_object(obj, 0).unwrap();
        let base = RgbaImage::from_pixel(513, 257, Rgba([220, 40, 90, 128]));
        let mut tiles = PaintTiles::new(513, 257);
        let tiled = tiles.publish(&base, false).unwrap();
        let mut whole = ImageLods::default();
        whole.set(
            2,
            ImageData {
                width: 513,
                height: 257,
                data: Blob::from(base.into_raw()),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
            },
        );
        let mut renders = Vec::new();
        for lods in [whole, tiled] {
            let mut text = crate::text::TextContext::new();
            let scene = crate::canvas::export_scene_of(
                &doc,
                &[object],
                Rect::new(0., 0., 512., 512.),
                1.,
                None,
                &HashMap::from([(asset, lods)]),
                false,
                &mut text,
                Color::BLACK,
            );
            let size = wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 1,
            };
            let texture = dev.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            renderer
                .render_to_texture(
                    &dev.device,
                    &dev.queue,
                    &scene,
                    &texture.create_view(&Default::default()),
                    &vello::RenderParams {
                        base_color: Color::WHITE,
                        width: 512,
                        height: 512,
                        antialiasing_method: vello::AaConfig::Area,
                    },
                )
                .unwrap();
            let buffer = dev.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 512 * 512 * 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = dev.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(2048),
                        rows_per_image: Some(512),
                    },
                },
                size,
            );
            dev.queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            dev.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            rx.recv().unwrap().unwrap();
            renders.push(buffer.slice(..).get_mapped_range().to_vec());
        }
        let max_error = renders[0]
            .iter()
            .zip(&renders[1])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max_error <= 3,
            "tile seams differ by {max_error} channel levels"
        );
    }

    #[test]
    fn small_edit_allocates_one_coverage_tile_and_reuses_other_preview_blobs() {
        let base = RgbaImage::new(1024, 512);
        let mut paint = PaintTiles::new(1024, 512);
        let first = paint.publish(&base, false).unwrap().tiles.unwrap();
        paint.put_pixel(100, 100, Rgba([255, 0, 0, 128]));
        assert_eq!(paint.ink.len(), 1);
        assert_eq!(
            paint.ink.values().map(|t| t.as_raw().len()).sum::<usize>(),
            256 * 256 * 4
        );
        let next = paint.publish(&base, false).unwrap().tiles.unwrap();
        let rebuilt = first
            .tiles
            .iter()
            .zip(&next.tiles)
            .filter(|(a, b)| a.image.data.id() != b.image.data.id())
            .count();
        assert_eq!(rebuilt, 1);
        assert!(paint.publish(&base, false).is_none());
        // Existing scene snapshots retain their immutable, original pixels.
        assert!(first.tiles[0].image.data.as_ref().iter().all(|b| *b == 0));
    }

    #[test]
    fn boundary_edit_refreshes_neighbor_gutters_and_partial_tiles_stay_in_bounds() {
        let base = RgbaImage::new(513, 257);
        let mut paint = PaintTiles::new(513, 257);
        let first = paint.publish(&base, false).unwrap().tiles.unwrap();
        paint.put_pixel(255, 100, Rgba([1, 2, 3, 255]));
        let next = paint.publish(&base, false).unwrap().tiles.unwrap();
        assert_eq!(
            first
                .tiles
                .iter()
                .zip(&next.tiles)
                .filter(|(a, b)| a.image.data.id() != b.image.data.id())
                .count(),
            2
        );
        assert_eq!(next.tiles.last().unwrap().core.width(), 1.0);
        assert_eq!(next.tiles.last().unwrap().core.height(), 1.0);
        for tile in &next.tiles {
            let r = tile.image_rect;
            assert!(r.x0 >= 0. && r.y0 >= 0. && r.x1 <= 513. && r.y1 <= 257.);
            assert_eq!(tile.image.width as f64, r.width());
            assert_eq!(tile.image.height as f64, r.height());
        }
        // The left pixel in the second tile is the first tile's edited rim.
        let neighbor = &next.tiles[1].image;
        let offset = (100 * neighbor.width as usize) * 4;
        assert_eq!(&neighbor.data.as_ref()[offset..offset + 4], &[1, 2, 3, 255]);
    }

    #[test]
    fn tiled_results_and_published_pixels_match_full_image_compositing() {
        let base = RgbaImage::from_fn(513, 257, |x, y| Rgba([x as u8, y as u8, 90, (x + y) as u8]));
        let mut dense = RgbaImage::new(513, 257);
        let mut paint = PaintTiles::new(513, 257);
        for y in 248..257 {
            for x in 248..513 {
                let ink = Rgba([20, 140, 240, ((x + y) % 256) as u8]);
                dense.put_pixel(x, y, ink);
                paint.put_pixel(x, y, ink);
            }
        }
        for erase in [false, true] {
            let mut expected = base.clone();
            if erase {
                for (p, m) in expected.pixels_mut().zip(dense.pixels()) {
                    p[3] = ((p[3] as u32 * (255 - m[3] as u32) + 127) / 255) as u8;
                }
            } else {
                image::imageops::overlay(&mut expected, &dense, 0, 0);
            }
            assert_eq!(paint.result(&base, erase), expected);
            // New snapshots use the same reference pixels, including gutters.
            paint.gpu.clear();
            let snapshot = paint.publish(&base, erase).unwrap().tiles.unwrap();
            for tile in &snapshot.tiles {
                for y in 0..tile.image.height {
                    for x in 0..tile.image.width {
                        let offset = ((y * tile.image.width + x) * 4) as usize;
                        let pixel = expected.get_pixel(
                            tile.image_rect.x0 as u32 + x,
                            tile.image_rect.y0 as u32 + y,
                        );
                        assert_eq!(&tile.image.data.as_ref()[offset..offset + 4], &pixel.0);
                    }
                }
            }
        }
    }
}

impl PaintTiles {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ink: HashMap::new(),
            dirty: HashSet::new(),
            gpu: Vec::new(),
        }
    }
    pub(super) fn width(&self) -> u32 {
        self.width
    }
    pub(super) fn height(&self) -> u32 {
        self.height
    }
    pub(super) fn get_pixel(&self, x: u32, y: u32) -> &Rgba<u8> {
        self.ink
            .get(&(x / TILE, y / TILE))
            .map(|t| t.get_pixel(x % TILE, y % TILE))
            .unwrap_or(&CLEAR)
    }
    pub(super) fn put_pixel(&mut self, x: u32, y: u32, color: Rgba<u8>) {
        assert!(x < self.width && y < self.height);
        if self.get_pixel(x, y) == &color {
            return;
        }
        let key = (x / TILE, y / TILE);
        self.ink
            .entry(key)
            .or_insert_with(|| {
                RgbaImage::new(
                    TILE.min(self.width - key.0 * TILE),
                    TILE.min(self.height - key.1 * TILE),
                )
            })
            .put_pixel(x % TILE, y % TILE, color);
        // A one-pixel sampling gutter makes neighboring tile edges filter
        // against the same pixels when zoomed, rotated, or scaled.
        for ty in y.saturating_sub(1) / TILE..=(y + 1).min(self.height - 1) / TILE {
            for tx in x.saturating_sub(1) / TILE..=(x + 1).min(self.width - 1) / TILE {
                self.dirty.insert((tx, ty));
            }
        }
    }
    fn composite(&self, base: &RgbaImage, x: u32, y: u32, erase: bool) -> Rgba<u8> {
        let mut pixel = *base.get_pixel(x, y);
        let ink = self.get_pixel(x, y);
        if erase {
            pixel[3] = ((pixel[3] as u32 * (255 - ink[3] as u32) + 127) / 255) as u8;
        } else {
            pixel.blend(ink);
        }
        pixel
    }
    pub(super) fn result(&self, base: &RgbaImage, erase: bool) -> RgbaImage {
        let mut result = base.clone();
        for (&(tx, ty), tile) in &self.ink {
            for (x, y, _) in tile.enumerate_pixels() {
                let (x, y) = (tx * TILE + x, ty * TILE + y);
                result.put_pixel(x, y, self.composite(base, x, y, erase));
            }
        }
        result
    }
    fn tile(&self, base: &RgbaImage, tx: u32, ty: u32, erase: bool) -> RasterTile {
        let (x, y) = (tx * TILE, ty * TILE);
        let (right, bottom) = ((x + TILE).min(self.width), (y + TILE).min(self.height));
        let (left, top) = (x.saturating_sub(1), y.saturating_sub(1));
        let (r, b) = ((right + 1).min(self.width), (bottom + 1).min(self.height));
        let touched = (top / TILE..=(b - 1) / TILE).any(|ty| {
            (left / TILE..=(r - 1) / TILE).any(|tx| self.ink.contains_key(&(tx, ty)))
        });
        let pixels = if touched {
            RgbaImage::from_fn(r - left, b - top, |px, py| {
                self.composite(base, left + px, top + py, erase)
            })
        } else {
            image::imageops::crop_imm(base, left, top, r - left, b - top).to_image()
        };
        RasterTile {
            core: vello::kurbo::Rect::new(x as f64, y as f64, right as f64, bottom as f64),
            image_rect: amalith_core::Rect::new(left as f64, top as f64, r as f64, b as f64),
            image: vello::peniko::ImageData {
                width: pixels.width(),
                height: pixels.height(),
                data: vello::peniko::Blob::from(pixels.into_raw()),
                format: vello::peniko::ImageFormat::Rgba8,
                alpha_type: vello::peniko::ImageAlphaType::Alpha,
            },
        }
    }
    /// Initialize the base preview once, then rebuild only dirty tiles.
    /// None means no pixels changed and the previous snapshot stays valid.
    pub(super) fn publish(&mut self, base: &RgbaImage, erase: bool) -> Option<ImageLods> {
        let columns = self.width.div_ceil(TILE);
        if self.gpu.is_empty() {
            for ty in 0..self.height.div_ceil(TILE) {
                for tx in 0..columns {
                    self.gpu.push(self.tile(base, tx, ty, erase));
                }
            }
        } else {
            if self.dirty.is_empty() {
                return None;
            }
            for &(tx, ty) in &self.dirty {
                self.gpu[(ty * columns + tx) as usize] = self.tile(base, tx, ty, erase);
            }
        }
        self.dirty.clear();
        Some(ImageLods {
            levels: Default::default(),
            tiles: Some(std::sync::Arc::new(RasterTiles {
                width: self.width,
                height: self.height,
                tiles: self.gpu.clone(),
            })),
        })
    }
}
