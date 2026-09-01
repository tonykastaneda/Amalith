//! Progressive image decode: a background worker that sends GPU rasters
//! from coarse to fine so Place can insert the object immediately.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use amalith_core::AssetId;
use vello::peniko::ImageData;

use crate::canvas;

/// Longest side at each refinement step. Last value matches Vello's atlas.
pub const LOD_SIDES: [u32; 3] = [256, 2048, canvas::GPU_ATLAS_MAX];

pub struct LodReady {
    pub asset: AssetId,
    pub gpu: ImageData,
    pub level: u8,
    pub key: String,
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
        let mut last = 0u32;
        for (level, &side) in LOD_SIDES.iter().enumerate() {
            let cap = side.min(job.native_max.max(1)).min(canvas::GPU_ATLAS_MAX);
            if cap == last {
                break;
            }
            last = cap;
            let gpu = match &job.source {
                LodSource::Path(p) => canvas::decode_path_max_side(p, cap),
                LodSource::Bytes(b) => canvas::decode_bytes_max_side(b, cap),
            };
            let Some(gpu) = gpu else {
                continue;
            };
            if ready
                .send(LodReady {
                    asset: job.asset,
                    gpu,
                    level: level as u8,
                    key: job.key.clone(),
                })
                .is_err()
            {
                return;
            }
        }
    }
}
