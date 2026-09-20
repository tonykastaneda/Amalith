# Compositor attribution

Source: https://github.com/robbietilton/Compositor

Reviewed revision: `37dbe59b3cf71184b016e4f2f4aa74eada533aec`.

Copyright (c) 2026 Wonder Assembly LLC. MIT license; see LICENSE beside
this notice. Include that license when distributing the derived code.

`crates/amalith-shell/src/app/brush_tip.rs` adapts the normalized Gaussian
falloff from `Compositor/Document/BrushStroke.swift`, `BrushRaster.falloff`.
The surrounding Rust coverage integration, bounds checks, and tests are
Amalith changes. This is a port of the falloff only, not Compositor's full
brush engine, tile storage, stroke accumulation, UI, or GPU implementation.
