# Amalith

Amalith is a free, open-source Illustrator-class vector editor written in Rust. Its GUI, future CLI, scripts, plugins, and agents share the same command engine rather than implementing separate editing paths.

The document uses an infinite pasteboard: artboards are named regions in document space, not the edge of the file. Objects and global layers can sit across artboards or outside them without hitting an Illustrator-style canvas wall.

## Status

Amalith is early in development.

Working today:

- Document model, undoable command engine, and `.amalith` save/load crates
- Native desktop app with a New Document panel, multiple artboards, and document tabs
- `Cmd+N` to create a document
- `Shift+O` for the artboard tool: move, resize, and Alt/Option-drag to duplicate artboards
- `V` to return to selection mode and exit the artboard tool
- Space-drag to pan, `Cmd`+`+`/`-` to zoom, and `Cmd+Space`-drag for scrubby zoom

Not yet implemented:

- Pen tool
- Object selection
- Fill and stroke editing
- SVG/PDF export UI
- A real CLI binary

## Build and run

This setup uses Homebrew's `rustup` toolchain:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test --workspace
cargo run -p amalith-app
```

## Crates

- `amalith-core` — document model, including IDs, units, geometry, artboards, layers, and objects.
- `amalith-commands` — shared command engine and undo/redo history.
- `amalith-io` — serialization to and from the open `.amalith` container format.
- `amalith-app` — native desktop GUI.

## License

Licensed under either the MIT License or the Apache License 2.0, at your option.
