# Compositor attribution

Source: https://github.com/robbietilton/Compositor

Copyright (c) 2026 Wonder Assembly LLC. MIT license; see LICENSE beside
this notice. Include that license when distributing the derived code.

Every Amalith file that translates or adapts Compositor code or behavior is
listed below, with the upstream revision it was taken from. Add a row when
porting anything new, and keep the upstream revision exact.

| Amalith file | Upstream source | Revision | What was taken |
| --- | --- | --- | --- |
| `crates/amalith-shell/src/app/brush_tip.rs` | `Compositor/Document/BrushStroke.swift` (`BrushRaster.falloff`) | `37dbe59b3cf71184b016e4f2f4aa74eada533aec` | The normalized Gaussian soft-tip falloff. Not the full brush engine, tile storage, stroke accumulation, UI or GPU code. |
| `crates/amalith-shell/src/app/raster_brush.rs` (Clone Stamp) | `Compositor/Document/EditorSession+Brush.swift` (clone behavior) | `37dbe59b3cf71184b016e4f2f4aa74eada533aec` | Clone Stamp interaction: Option-click source point, aligned/non-aligned offset, sampling the un-stroked base. |
| `crates/amalith-shell/src/app/pixel_transform.rs` | `Compositor/Document/FloatingSelection.swift` | `37dbe59b3cf71184b016e4f2f4aa74eada533aec` | The lift → transform → bake flow for selected pixels. The resampler is Amalith's own. |

The surrounding Rust integration (commands, undo, rendering, bounds
checks and tests) is Amalith's own in every case. See
[docs/compositor-porting.md](../../docs/compositor-porting.md) for how each
port was adapted.
