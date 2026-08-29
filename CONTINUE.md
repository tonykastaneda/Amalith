# Amalith — session handoff

Read this first after `/compact` or `/new`. Product spec: `amalith-project-brief.md`.

**Repo:** `~/Documents/GitHub/Amalith`  
**GitHub:** `tonykastaneda/Amalith`  
**Working branch:** `main` only.  
**`dev-test`:** reference material only. Read it. Do not merge it, cherry-pick it, `checkout` its files onto `main`, or make `main` look like the spike — **unless Tony explicitly names what to pull from `dev-test` onto `main`.** A general “implement tools” or “use the spike as an example” is not that instruction. Implement new work on `main` from scratch; use `dev-test` the same way we use Graphite: a *how* example, not a source tree to copy.

**Run:**

```bash
cd ~/Documents/GitHub/Amalith
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo run -p amalith-app
```

You are the **orchestrator**. Do not implement app code yourself. Delegate to Herdr specialists in workspace **Amalith**. Writable path is Amalith only.

## Invariants

- GUI/CLI/agents/plugins all mutate through `Editor::execute`. No `document.objects[i] = …`.
- One command can be a **group of edits** (one undo). See `amalith-commands` history.
- Infinite pasteboard. Artboards are regions, not the file edge. No Illustrator canvas wall.
- Artboards do not parent objects. “On this board” = geometric bounds intersection.
- Native format: `.amalith` (zip + json). Not `.ai`.

## Crates

| Crate | Role |
|---|---|
| `amalith-core` | Document, IDs, units, transforms, artboards, layers, objects |
| `amalith-commands` | `Command` + undo/redo (`Editor`) |
| `amalith-io` | `.amalith` save/load |
| `amalith-app` | egui desktop GUI |

## Herdr panes (workspace Amalith)

| Pane | Agent | Owns |
|---|---|---|
| ORCH | this Grok | orchestration only |
| ENGINE | Claude `doc-engine` | core / commands / io |
| ARTBOARDS | Codex `artboards` | New Document, Shift+O, panel, tabs, artboard move/duplicate |
| UI-INTERACTION | Codex `ui-interaction` | camera, pan/zoom, M, V, object transform |
| DOCS | Codex `readme` | README / naming |

## Done (in the working tree — much of it uncommitted)

- New Document panel; document tabs; Cmd+N
- Artboards: Shift+O, move/resize, Option-drag **DuplicateArtboard** (copies intersecting artwork), **MoveArtboard** (moves intersecting artwork with the board, one undo)
- Artboards panel: index, name, double-click rename, `+`
- Panel docks left / right / floats (drag title bar)
- Camera: Space pan, Cmd+/− zoom, Cmd+Space scrubby zoom, trackpad pan + pinch
- **M** rectangle tool → `CreateRect`; paths draw on canvas
- **V** selection: hit-test, blue box, 8 handles, center square, move, scale handles (`SetTransform`), rotate (just outside corners; hit zone was too small — enlarge if still fiddly)
- V does not rotate artboards

## Next fundamentals (do in this order, main tree, not worktrees)

1. Confirm rotate hit zone is actually findable (just **outside** corner handles).
2. **Cmd+S / Cmd+O** in the GUI (engine already roundtrips `.amalith`).
3. Then: Pen, fill/stroke, real CLI — only after New → M → V → move/rotate → undo → save → reopen.

Do **not** spawn parallel feature worktrees until that loop works.

## Command cheat sheet (engine)

`CreateArtboard` `DeleteArtboard` `RenameArtboard` `ResizeArtboard` `MoveArtboard` `DuplicateArtboard`  
`CreateLayer` `CreateRect` `MoveObject` `SetTransform`

## Shortcuts

| Key | Action |
|---|---|
| M | Rectangle tool |
| V | Selection (objects) |
| Shift+O | Artboard tool |
| Space | Hand pan |
| Cmd+Space | Scrubby zoom |
| Cmd+/− | Zoom |
| Option-drag artboard | Duplicate board + contents |
| Cmd+N | New document |

## Orchestrator rules

- Product work is **`main`**. **`dev-test` is not a feature branch to land.** It is extra reference, like Graphite.
- Never dump, merge, or file-checkout the `dev-test` tree onto `main` unless Tony explicitly says to pull a named piece from `dev-test` onto `main`. If a tool from the spike is needed without that instruction, re-implement it on `main` through the command engine.
- One specialist at a time on `main.rs` when possible (ARTBOARDS vs UI-INTERACTION collide there).
- Do not launch the GUI unless asked. Do not double-open `amalith-app`.
- After compact: read this file, then `git status` and `amalith-project-brief.md`.
- **Graphite** (`https://github.com/GraphiteEditor/Graphite`): Rust implementation reference for tools/viewport/select. Study it when implementing those. Do **not** become Graphite (node graph is their product; Illustrator-class documents are ours). Apache 2.0 if copying.
