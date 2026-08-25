//! Byte store for embedded assets, keyed by their container-relative path
//! (e.g. `"images/photo-001.png"`, matching
//! [`amalith_core::AssetSource::Embedded::container_path`]).
//!
//! `amalith-core::Asset` only tracks *where* embedded bytes live inside the
//! container, not the bytes themselves — the document model has no reason
//! to hold raster payloads in memory just to answer "what objects exist".
//! `AssetStore` is the companion the caller supplies to
//! [`crate::save`]/receives from [`crate::load`] holding those payloads.
use std::collections::HashMap;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AssetStore(HashMap<String, Vec<u8>>);

impl AssetStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, container_path: impl Into<String>, bytes: Vec<u8>) {
        self.0.insert(container_path.into(), bytes);
    }

    pub fn get(&self, container_path: &str) -> Option<&[u8]> {
        self.0.get(container_path).map(Vec::as_slice)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
