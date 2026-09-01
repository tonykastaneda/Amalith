//! Image LODs: a background worker that decodes GPU rasters coarse → fine
//! (Place stays instant) and a picker that draws the smallest copy whose
//! long side covers the object on screen.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use amalith_core::AssetId;
use vello::peniko::ImageData;

use crate::canvas;

/// Longest side at each refinement step. Last value matches Vello's atlas.
pub const LOD_SIDES: [u32; 3] = [256, 2048, canvas::GPU_ATLAS_MAX];

/// GPU copies for one asset, coarse → fine. Paint picks by on-screen size.
#[derive(Clone, Default)]
pub struct ImageLods {
    pub levels: [Option<ImageData>; 3],
}

impl ImageLods {
    pub fn set(&mut self, level: u8, gpu: ImageData) {
        if let Some(slot) = self.levels.get_mut(level as usize) {
            *slot = Some(gpu);
        }
    }

    /// Smallest available level whose long side covers `cover_px` on screen.
    pub fn pick(&self, cover_px: f64) -> Option<&ImageData> {
        let sides = [
            self.levels[0].as_ref().map(|i| i.width.max(i.height)),
            self.levels[1].as_ref().map(|i| i.width.max(i.height)),
            self.levels[2].as_ref().map(|i| i.width.max(i.height)),
        ];
        let i = pick_level(cover_px, sides)?;
        self.levels[i].as_ref()
    }
}

/// Choose LOD index from on-screen coverage and available long sides.
pub fn pick_level(cover_px: f64, long_sides: [Option<u32>; 3]) -> Option<usize> {
    let need = cover_px.ceil().max(1.0) as u32;
    let mut best = None;
    for (i, side) in long_sides.iter().enumerate() {
        let Some(s) = *side else {
            continue;
        };
        best = Some(i);
        if s >= need {
            return Some(i);
        }
    }
    best
}

pub struct LodReady {
    pub asset: AssetId,
    /// `None` when this step failed; `done` is still sent so inflight clears.
    pub gpu: Option<ImageData>,
    pub level: u8,
    pub key: String,
    /// Last step for this job (small images may never reach 8192).
    pub done: bool,
}

enum LodSource {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

struct LodJob {
    asset: AssetId,
    key: String,
    native_max: u32,
    source: LodSource,
}

pub struct LodHub {
    jobs: Sender<LodJob>,
    ready: Receiver<LodReady>,
}

impl LodHub {
    pub fn new() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel::<LodJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<LodReady>();
        let _ = std::thread::Builder::new()
            .name("amalith-lod".into())
            .spawn(move || worker(jobs_rx, ready_tx));
        Self {
            jobs: jobs_tx,
            ready: ready_rx,
        }
    }

    pub fn enqueue_path(&self, asset: AssetId, path: PathBuf, native_w: u32, native_h: u32) {
        let key = path.to_string_lossy().into_owned();
        let _ = self.jobs.send(LodJob {
            asset,
            key,
            native_max: native_w.max(native_h),
            source: LodSource::Path(path),
        });
    }

    pub fn enqueue_bytes(&self, asset: AssetId, key: String, bytes: Vec<u8>, native_w: u32, native_h: u32) {
        let _ = self.jobs.send(LodJob {
            asset,
            key,
            native_max: native_w.max(native_h),
            source: LodSource::Bytes(bytes),
        });
    }

    pub fn drain(&self) -> Vec<LodReady> {
        let mut out = Vec::new();
        loop {
            match self.ready.try_recv() {
                Ok(v) => out.push(v),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

fn worker(jobs: Receiver<LodJob>, ready: Sender<LodReady>) {
    while let Ok(job) = jobs.recv() {
        let mut caps = Vec::new();
        let mut last = 0u32;
        for (level, &side) in LOD_SIDES.iter().enumerate() {
            let cap = side.min(job.native_max.max(1)).min(canvas::GPU_ATLAS_MAX);
            if cap == last {
                continue;
            }
            last = cap;
            caps.push((level as u8, cap));
        }
        let n = caps.len();
        for (i, (level, cap)) in caps.into_iter().enumerate() {
            let gpu = match &job.source {
                LodSource::Path(p) => canvas::decode_path_max_side(p, cap),
                LodSource::Bytes(b) => canvas::decode_bytes_max_side(b, cap),
            };
            if ready
                .send(LodReady {
                    asset: job.asset,
                    gpu,
                    level,
                    key: job.key.clone(),
                    done: i + 1 == n,
                })
                .is_err()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_out_picks_256() {
        assert_eq!(
            pick_level(80.0, [Some(256), Some(2048), Some(8192)]),
            Some(0)
        );
    }

    #[test]
    fn mid_zoom_picks_2048() {
        assert_eq!(
            pick_level(1000.0, [Some(256), Some(2048), Some(8192)]),
            Some(1)
        );
    }

    #[test]
    fn zoomed_in_picks_8192() {
        assert_eq!(
            pick_level(4000.0, [Some(256), Some(2048), Some(8192)]),
            Some(2)
        );
    }

    #[test]
    fn missing_high_falls_back_to_what_we_have() {
        assert_eq!(pick_level(4000.0, [Some(256), None, None]), Some(0));
    }

    #[test]
    fn exact_side_stays_on_that_level() {
        assert_eq!(
            pick_level(256.0, [Some(256), Some(2048), Some(8192)]),
            Some(0)
        );
    }

    #[test]
    fn one_pixel_over_steps_up() {
        assert_eq!(
            pick_level(256.1, [Some(256), Some(2048), Some(8192)]),
            Some(1)
        );
    }

    #[test]
    fn nothing_decoded_yet() {
        assert_eq!(pick_level(4000.0, [None, None, None]), None);
    }
}
