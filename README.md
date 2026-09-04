<p align="center">
  <img src="website/public/brand/amalith-wordmark.png" width="360" alt="Amalith logo">
</p>

<p align="center"><em>The Illustrator-class vector editor that launched yesterday with 20 years of tutorials.</em></p>

<p align="center">
  <a href="https://www.amalith.app/">Website</a> ·
  <a href="amalith-project-brief.md">Project brief</a> ·
  <a href="#build-and-run">Build</a>
</p>

---

Amalith is a free, open-source, cross-platform professional vector design app. It aims to feel immediately familiar to an Illustrator user — same tools, same shortcuts, same muscle memory — while staying independent, open, and free of any subscription or cloud lock-in. The native editor and its design engine are Rust-based; the project also includes a Next.js/TypeScript website.

Two goals, one architecture:

- **Familiar to designers.** Open your work and keep designing without relearning vector graphics.
- **Drivable without the mouse.** The GUI is a client of a shared command engine — the same operations a tool issues can be issued by a script, a plugin, the CLI, or an agent, and undo/redo works the same for all of them.

The document uses an **infinite pasteboard**: artboards are named regions in document space, not the edge of the file. Objects and global layers can sit across artboards or outside them with no canvas wall.

## Status

Early development, macOS first. What works today:

- **Core** — document model, undoable command engine, `.amalith` container (zip + JSON) save/load
- **App shell** — winit + wgpu + vello, a custom dockable panel system (tear panels off into their own OS windows), document tabs, and a Home / welcome screen
- **Tools** — Selection, Direct Selection (hold <kbd>Space</kbd> to peek every node), Pen, Rectangle / Rounded Rectangle / Ellipse / Polygon / Star, Artboard, and **Type**
- **Text** — point and area type with a live editor (caret, selection, IME) and a **Character panel** (font family / style, size, leading, tracking, under/strikethrough, small caps, sub/superscript)
- **Appearance** — fill & stroke paint, colour picker, stroke cap / join / dash flyout
- **Editing** — grouping, duplicate, z-order, marquee & shift-click select, transform handles
- **Clipboard** — copy/paste through the OS clipboard as standalone SVG (round-trips with Illustrator, Figma, browsers); Illustrator SVG import
- **Panels** — Tools, Layers, Artboards, Swatches, Character — dockable, with a Panels menu to show/hide them
- **macOS** — native menu bar, Preferences (<kbd>⌘,</kbd>), hover tooltips, runtime app icon
- **Canvas** — multi-artboard New Document dialog, pan / zoom / scrubby zoom, the infinite pasteboard

Not yet:

- A standalone CLI binary (the command engine is CLI-ready; the binary isn't built)
- SVG / PDF export UI
- Per-character text styling (styles are whole-object for now)
- Settings persistence across launches
- Windows / Linux polish
- ICC profiles, advanced colour management, and print workflows

## Build and run the native editor

Rust stable (via `rustup`). On macOS with Homebrew's toolchain:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test --workspace
cargo run -p amalith-shell
```

Pressing <kbd>T</kbd> gives you the Type tool; <kbd>V</kbd> returns to Selection. The menu bar and <kbd>⌘,</kbd> are macOS-only.

## Website development

The marketing site is a separate Next.js/TypeScript project in `website/`:

```bash
cd website
npm install
npm run dev
```

It deploys to GitHub Pages from `main` via `.github/workflows/pages.yml`: <https://www.amalith.app/>

The native editor owns document data, geometry, commands, rendering, and file formats. The website is intentionally a separate presentation layer. For colour management, Amalith will rely on established professional libraries and domain expertise rather than attempting to reinvent ICC and print-colour science in the application.

## Crates

| Crate | Role |
|---|---|
| `amalith-core` | Document model — IDs, units, geometry, transforms, artboards, layers, objects |
| `amalith-commands` | The shared `Command` vocabulary and undo/redo `Editor` — the single mutation path |
| `amalith-io` | Serialization to and from the open `.amalith` container format, plus SVG import/export |
| `amalith-shell` | The native desktop GUI (winit + wgpu + vello; no widget toolkit) |

The invariant everything hangs on: **the GUI never mutates the document directly.** Every user-facing change — from a tool, a shortcut, a script, or an agent — goes through `Editor::execute`, so undo, plugins, and automation all share one vocabulary.

## License

Licensed under either the MIT License or the Apache License 2.0, at your option.
