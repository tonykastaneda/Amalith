# Amalith — Project Brief

> **The vector editor that launched yesterday with 20 years of tutorials.**

## Vision

Amalith is a **free, public, open-source, cross-platform professional vector design application** intended to make Illustrator-class vector design accessible to everyone.

The goal is not to create another Inkscape-style alternative that requires designers to relearn their workflow. The goal is to build a modern vector editor that feels immediately familiar to experienced Illustrator users while remaining independent, open, and free.

**North-star tests:**

> Can an Illustrator designer install Amalith, open their work, and start designing without relearning vector graphics?

> Can that same designer put the mouse down and drive the identical operations from the CLI, a script, a plugin, or an agent?

Amalith is the Illustrator-class editor that launched yesterday with 20 years of tutorials — and the one where the GUI and the CLI are the same product, not two different programs.

## Core Philosophy

- Free to use
- Public repository
- Open source
- Cross-platform: macOS, Windows, Linux
- No subscription
- No mandatory account
- No proprietary cloud dependency
- Open native document format
- Familiar Illustrator-style workflow conventions
- Infinite pasteboard — artboards are pages, the document does not have a canvas wall
- Professional rather than “good enough for open source”
- Automation, plugins, CLI, and agents treated as first-class capabilities
- The GUI is a client of the command engine, never a private back door

## What makes Amalith Amalith

This is not optional polish. It is the product.

Most vector editors pick one audience: a GUI clone of Illustrator, *or* a scriptable/headless toolchain. Amalith is both, on purpose, with one operation layer:

```text
mouse, shortcut, plugin, script, CLI, agent
                    ↓
             COMMAND ENGINE
                    ↓
              DOCUMENT MODEL
```

If a designer can do it in the GUI, they must be able to do it from the CLI. If a script can do it, undo/redo must still work. If an agent can do it, it is the same command the Pen tool issued.

**Invariants — do not break these:**

- The GUI must never own or mutate document logic. It sends commands.
- Every user-facing mutation goes through the command engine so undo, plugins, scripts, CLI, agents, and tests share one vocabulary.
- New features are not done when the panel works. They are done when the command exists, is undoable, and is callable without the GUI.
- UI technology may change. The command engine and document model stay independent of it.
- Do not grow a second, GUI-only state model “just for the canvas.” The canvas displays the document; it does not secretly become the document.
- The pasteboard is infinite. Artboards are named rectangles in document space, not the edges of the file. Do not copy Illustrator’s visible canvas ceiling (the big square at extreme zoom). Camera zoom clamps are view limits, not document bounds. Renderer tiling later is a cache, not a wall the user can hit.

This is what keeps Amalith from becoming “another Illustrator skin” or “another headless SVG tool.” Same muscle memory in the GUI. Same power in the CLI.

## Compatibility Strategy

### Illustrator is the workflow benchmark

Amalith should preserve the interaction conventions designers already know wherever practical:

- `V` — Selection
- `A` — Direct Selection
- `P` — Pen
- `T` — Type
- `M` — Rectangle
- `L` — Ellipse
- `I` — Eyedropper
- `G` — Gradient
- `Shift+M` — Shape Builder-style workflow
- `Shift+O` — Artboard
- Space — pan
- Shift — constrain
- Option/Alt — duplicate or modify handles where expected
- Cmd/Ctrl conventions

The same principle applies beyond shortcuts: selection behavior, Bézier handles, snapping, smart guides, transforms, layers, artboards, clipping masks, compound paths, Pathfinder-style operations, gradients, typography, embedded images, and export behavior should feel familiar.

This allows designers to benefit from decades of existing vector-design and Illustrator education rather than requiring a completely new tutorial ecosystem.

## Inkscape Strategy

Inkscape's public repository represents decades of solved vector-editor engineering.

Amalith can study and, where license-compatible, adapt or port relevant systems and lessons into the new Rust architecture.

Inkscape can help inform areas such as:

- Bézier/path manipulation
- SVG behavior
- Snapping edge cases
- Geometry
- Transforms
- Gradients
- Text
- Filters
- Import/export
- Rendering decisions
- Selection behavior

Amalith should **not simply become Inkscape rewritten in Rust**.

The model is:

```text
INKSCAPE
existing engineering / edge cases
        ↓
study / adapt / port
        ↓
     AMALITH
        ↑
Illustrator workflow expectations
```

Because Amalith is intended to remain free and open source, a GPL-compatible licensing strategy can be considered where direct GPL-derived work is used. License provenance, notices, attribution, and source requirements should still be tracked carefully.

## Technology

### Primary language: Rust

Rust should own the actual design engine:

```text
Rust
├── document model
├── vector geometry
├── Bézier engine
├── boolean/path operations
├── snapping
├── hit testing
├── typography
├── raster/image handling
├── asset management
├── undo/redo
├── serialization
├── import/export
├── command system
└── rendering
```

### Rendering

Use a GPU-oriented renderer, with `wgpu` as a strong candidate for the graphics abstraction layer.

### UI

Two reasonable directions:

1. Rust-native/custom UI for maximum control.
2. Rust core + Tauri/TypeScript/React for faster UI development and easier agent-assisted implementation.

The UI technology can change. **The document and graphics engine should remain Rust and independent of the UI.**

## Architecture

The GUI should never directly own or mutate document logic. It is one client among equals.

```text
Pen Tool ───────────┐
Keyboard Shortcut ──┤
Plugin ─────────────┤
Script ─────────────┤
AI Agent ───────────┤
CLI ────────────────┘
                    ↓
             COMMAND ENGINE
                    ↓
              DOCUMENT MODEL
                    ↓
                 RENDERER
```

For example, a UI operation should conceptually become something like:

```rust
document.execute(
    Command::BooleanUnion {
        objects: selection,
    }
);
```

rather than the UI directly changing internal object state.

This provides one consistent operation layer for:

- GUI tools
- keyboard shortcuts
- plugins
- scripts
- CLI commands
- AI agents
- undo/redo
- testing

## Document Model

The document model is one of the systems that should be deliberately designed rather than casually generated and repeatedly rewritten by coding agents.

Conceptually:

```text
Document
├── Metadata
├── Artboards
├── Layers
├── Objects
├── Assets
├── Swatches
└── Settings
```

Objects may include:

```text
Object
├── Path
├── Text
├── Image
├── Group
├── CompoundPath
└── Symbol
```

Each object can then have geometry, transforms, appearance, opacity, etc.

Stable identifiers should exist from the beginning:

```text
ArtboardId
ObjectId
LayerId
AssetId
```

Coordinate systems, transforms, units, bounds, stacking order, ownership, and serialization semantics should be defined early.

## Open Native File Format

Amalith should not depend on `.ai` as its native format.

A Amalith document could be an open ZIP-style container such as:

```text
design.amalith
│
├── document.json
├── artwork/
│   ├── artboard-001.json
│   └── artboard-002.json
├── images/
│   ├── photo-001.png
│   └── texture-001.jpg
├── profiles/
│   └── coated.icc
├── fonts/
└── preview.png
```

The format should support both linked and embedded raster assets.

Conceptually:

```json
{
  "type": "image",
  "source": "./Links/product-photo.psd",
  "embedded": false,
  "transform": {},
  "crop": {},
  "opacity": 1
}
```

Embedding would copy the asset into the document container.

The format should be documented publicly so other applications, plugins, scripts, and tools can read and write Amalith documents.

## Interchange Formats

Long-term compatibility targets:

- SVG — excellent
- PDF — excellent
- EPS — strong where practical
- PSD — interoperability where practical
- AI — best-effort import/interchange

Adobe's `.ai` format should be treated as an interoperability problem, not as the foundation of Amalith.

## Feature Target

The objective is not immediate 100% Illustrator feature parity. The first major target is:

> **Can a working graphic designer spend an entire normal workday in Amalith without opening Illustrator?**

High-priority Illustrator-class functionality includes:

- Selection
- Direct Selection
- Pen tool
- Pencil/brush workflows
- Bézier node editing
- Shapes
- Shape Builder-style operations
- Boolean/Pathfinder operations
- Compound paths
- Clipping masks
- Fill/stroke
- Multiple fills/strokes / appearance stack
- Gradients
- Transparency
- Blend modes
- Artboards
- Layers/groups
- Guides
- Smart guides/snapping
- Align/distribute
- Transform/rotate/scale
- Text
- OpenType/variable fonts
- Text on path
- Area text
- Character/paragraph styles
- Images: linked and embedded
- Symbols
- Swatches
- Styles
- PDF/SVG export
- Professional color/print capabilities over time

Lower-priority features can come later, such as specialized 3D, perspective tools, puppet warp, and other less frequently used Illustrator functionality.

## AI and Automation

AI should **not be the product**. Amalith should first be an excellent vector editor.

However, every meaningful operation should eventually be exposed through a stable command API:

```text
select()
move()
scale()
rotate()
setFill()
setStroke()
createPath()
booleanUnion()
embedImage()
createArtboard()
outlineText()
exportPDF()
```

That enables:

- GUI interaction
- plugins
- scripting
- CLI workflows
- headless rendering
- automation
- agent-controlled editing

Potential future surfaces include:

```text
Desktop app
CLI
Plugin SDK
Agent API
Headless renderer
Web/WASM version
```

All should share the same Rust core.

## Vibe-Coding Strategy

Coding agents can radically reduce the implementation cost of a project with this much surface area, but they should work against a deliberately designed architecture and specifications.

A useful repository structure could include behavioral specifications:

```text
/specs
    pen-tool.md
    direct-selection.md
    snapping.md
    pathfinder.md
    gradients.md
    typography.md
    appearance.md
    artboards.md
```

Agents can then own bounded subsystems:

```text
Agent 01 → Pen tool
Agent 02 → snapping engine
Agent 03 → SVG importer
Agent 04 → PDF exporter
Agent 05 → gradients
Agent 06 → typography
Agent 07 → boolean operations
Agent 08 → regression tests
Agent 09 → UI
Agent 10 → performance
```

The principle is:

> **Architecture and specifications are deliberate; implementation can be heavily agent-assisted.**

Behavior discovered while studying mature applications should become automated tests wherever possible.

## Where Development Should Start

Do **not** start by recreating Illustrator's New Document dialog.

Start one level below it.

```text
1. Document Core
      ↓
2. Command + Undo/Redo
      ↓
3. Artboards
      ↓
4. Canvas / Camera
      ↓
5. Basic Objects
      ↓
6. Selection + Transform
      ↓
7. New Document UI
```

### Phase 1 — Document Core

Define what a Amalith document is before building significant UI.

Establish:

- Document structure
- IDs
- coordinates
- units
- transforms
- bounds
- stacking order
- object ownership
- serialization

### Phase 2 — Command + History System

Tools should not directly mutate documents.

Avoid:

```rust
document.objects[5].x = 300.0;
```

Prefer:

```rust
execute(Command::MoveObject {
    object,
    delta,
});
```

Commands feed document mutation and history, providing consistent undo/redo and future automation support.

### Phase 3 — Artboards

Artboards should be the first major user-facing system.

Initial functionality:

- Create
- Delete
- Duplicate
- Move
- Resize
- Rename
- Reorder
- Preset sizes
- Custom sizes
- px / pt / in / mm / cm
- Portrait/landscape
- Multiple artboards
- Infinite pasteboard
- Artboard-aware coordinates

### Phase 4 — Canvas / Camera

Implement:

- GPU-backed canvas
- infinite pasteboard
- pan
- zoom
- multiple visible artboards

### Phase 5 — First Object

Put one rectangle on an artboard.

### Phase 6 — Selection and Transform

Implement enough of the Selection tool to select, move, and transform the rectangle.

### Phase 7 — New Document UI

Only after the underlying document/artboard systems work should the polished New Document experience be built.

## Milestone 0.1

The first complete vertical slice should be intentionally small:

```text
File → New

1920 × 1080 px

        ↓

New Document

        ↓

Artboard appears

        ↓

Press M

        ↓

Drag rectangle

        ↓

Press V

        ↓

Select rectangle

        ↓

Move it

        ↓

Cmd/Ctrl + Z

        ↓

Rectangle moves back

        ↓

Save

        ↓

Quit Amalith

        ↓

Open file

        ↓

Everything is exactly where it was
```

This tiny workflow exercises nearly the entire foundational architecture:

- UI
- keyboard input
- tools
- camera
- renderer
- geometry
- document model
- artboards
- objects
- selection
- commands
- undo/redo
- serialization
- filesystem

Once this works reliably, development can move toward:

```text
Selection
    ↓
Direct Selection
    ↓
Bézier Paths
    ↓
Pen Tool
```

That is where Amalith begins proving whether it can actually reproduce the interaction quality and muscle memory expected from an Illustrator-class editor.

## One-Sentence Product Definition

> **Amalith is a free, open-source, cross-platform Illustrator-class vector editor built in Rust around an open document format, a command engine, and familiar professional workflows — the vector editor that launched yesterday with 20 years of tutorials, where the GUI and the CLI are the same product.**
