//! amalith-script: a headless ExtendScript compatibility shim.
//!
//! Runs unmodified Adobe Illustrator `.jsx` automation scripts against
//! Amalith's own document engine (`amalith-core`/`amalith-commands`)
//! instead of real Illustrator, retargeting file I/O to Amalith-native
//! formats instead of `.ai`.
//!
//! # Governing principle: every collection is a live query
//!
//! Every JS-facing collection (`doc.placedItems`, `layer.pageItems`,
//! `doc.artboards`, ...) and every JS-facing item wrapper (`PlacedItem`,
//! `PageItem`, ...) holds only an id into the real `Editor`/`Document` —
//! never a cached snapshot. Illustrator's own well-known scripting hazards
//! (an item's `.typename` changing live after `.embed()`, a collection
//! shrinking mid-iteration after `.remove()`) fall out of this for free:
//! the shim always reflects current state, exactly like real Illustrator,
//! and the burden of snapshotting a list before mutating its members stays
//! on the script, exactly as it would against the real application.

pub mod engine;
pub mod host;
pub mod outline;
pub mod run;

pub use run::run_pipeline;
