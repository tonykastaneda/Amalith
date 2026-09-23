---
name: amalith
description: "Inspect and automate the Amalith design document belonging to the editor that spawned this shell. Use only when AMALITH_ENV=1, which means this shell was started from Amalith's built-in terminal and a design document is in reach. Covers reading the saved .amalith container and running ExtendScript (.jsx) automation. Do not use merely because a task involves vector art, design files or images."
---

# Amalith

Amalith is a vector design application with a built-in terminal. If you are
reading this from inside that terminal, there is a design document in reach.

Before anything else, verify that this shell really was spawned by Amalith:

```bash
test "${AMALITH_ENV:-}" = 1
```

If the check fails, say that you are not running inside Amalith and stop. Do
not go looking for `.amalith` files elsewhere on the disk.

## What the environment tells you

| Variable | Meaning |
| --- | --- |
| `AMALITH_ENV` | `1` inside an Amalith terminal. The only trigger for this skill. |
| `AMALITH_VERSION` | Version of the app that spawned this shell, e.g. `0.0.3`. |
| `AMALITH_DOCUMENT_PATH` | The open document's `.amalith` file. **Unset for a document that has never been saved.** |
| `AMALITH_SCRIPTS_DIR` | The user's scripts folder. The shell starts in it. Unset if they have not chosen one. |
| `AMALITH_SKILL` | This file's path, for agents with no skill system of their own. |

Check a variable is actually set before relying on it. An unset
`AMALITH_DOCUMENT_PATH` means an unsaved document, not an error — say so and
ask the user to save if you need to read it.

## The limitation that matters most

**There is no live connection to the running editor.** `AMALITH_DOCUMENT_PATH`
points at the document *as it was last saved*, not as it looks on screen right
now. Consequences:

- Unsaved edits are invisible to you. If what you read looks out of date, ask
  the user to save (⌘S) rather than guessing or working from stale geometry.
- You cannot change what the user is looking at. Writing to the `.amalith` file
  does **not** update the open window, and their next save will overwrite
  whatever you wrote.
- Nothing you do appears in the editor's undo history, so the user cannot ⌘Z
  your work.

So treat the document as **read-only**. If the user wants a modified version,
write it to a new path and tell them where it went — never overwrite the file
they have open.

## Reading a document

`.amalith` is an open zip container, so ordinary tools work:

```bash
unzip -l "$AMALITH_DOCUMENT_PATH"                          # list entries
unzip -p "$AMALITH_DOCUMENT_PATH" document.json            # document-wide data
unzip -p "$AMALITH_DOCUMENT_PATH" 'artwork/layer-*.json'   # artwork per layer
```

The layout:

- `document.json` — `format_version`, metadata, settings, artboards, swatches,
  gradients, guides, the asset table, the layer list and the symbol list.
- `artwork/layer-<layer-id>.json` — the objects on one layer. The layer ids come
  from the layer list in `document.json`.
- `artwork/symbol-<symbol-id>.json` — one symbol definition's objects.
- Everything else is embedded asset bytes, mostly images.

Geometry is always stored in canonical px, whatever display unit the document
is set to, so do not apply a unit conversion to the numbers you read.

## Automating with .jsx

Amalith ships a headless ExtendScript compatibility shim, so Illustrator-style
`.jsx` automation runs against its document engine:

```bash
amalith-script run script.jsx [script2.jsx ...]
```

Scripts run sequentially in one shared context, so `$.global` and any documents
a script opened persist from one file to the next — matching how a chained
Illustrator pipeline behaves. Note that `amalith-script` takes **script paths
only**; a script opens and saves documents itself, so passing a `.amalith` path
as an argument will not work.

Scripts saved in `$AMALITH_SCRIPTS_DIR` also show up under File ▸ Scripts in
the app, so anything you write there is reachable from the menu bar.

`amalith-script` runs headlessly and does not touch the open window. Have it
write to a new file rather than over the user's document.

## Working with the user

- Prefer reading and reporting to changing anything.
- Name the specific layers, objects or artboards you looked at, so the user can
  check your reasoning against what is on their canvas.
- When you need current state, ask for a save rather than assuming the file is
  current.
