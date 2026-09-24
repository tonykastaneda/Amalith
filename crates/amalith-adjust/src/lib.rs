//! `amalith-adjust`: the pixel math behind adjustment layers.
//!
//! Headless and GPU-free, so it's fast to test. The shell's GPU pass
//! applies the lookup tables built here; [`cpu::apply`] is the reference
//! it's checked against. The parameter types live in
//! `amalith_core::adjustment`.

pub mod blend;
pub mod color;
pub mod cpu;
pub mod lut;

pub use lut::{compile, ColorLut, CUBE_N};
