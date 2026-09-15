# Canvas panes

Press **⌘B** to enter pane prefix mode. The bottom bar displays available commands:

- **D** splits the focused pane into two equal side-by-side views and focuses the new chooser.
- **Left / Right**, or **Tab / Shift-Tab**, changes pane focus.
- **Escape** cancels prefix mode.

Paste in Back remains in the Edit menu without the former ⌘B shortcut.

A new pane offers **Create New Document**, **Open Terminal**, and the open-document list. The list scrolls when it exceeds the pane height. Pick an already-visible document to open another view of it. The document and undo history are shared, while each pane keeps its own pan and zoom. Click a document pane to focus it and interact with the artwork; the accent border indicates where canvas keyboard commands go. Wheel/pinch navigation is restricted to the focused canvas.

The **⋯** at the right of a document pane header returns that pane to the chooser. Documents remain open in the global tab strip. Creating/opening a document or choosing a document tab puts it in the focused pane. Closing a document makes its other views show the chooser.

Each terminal pane runs its own PTY. Changing pane focus does not restart shells. Closing the terminal's header tab returns to the chooser and keeps that shell available through Open Terminal in that pane. File → Scripts → Terminal also opens a terminal through the same pane system. Existing visible or hidden legacy terminal sessions are migrated without restarting. Quitting terminates all sessions.

The first implementation uses horizontal, equal splits with a minimum width check; pane layout and terminal sessions are not serialized across application restarts. Document files remain unchanged.

## Implementation and verification

`multiplexer.rs` owns a binary pane tree, stable pane IDs, document references, and pane-local camera state. `app/multiplexer.rs` resolves stable document IDs to the existing tab/editor ownership, synchronizes the focused camera, manages terminal focus, and paints inactive document views directly from their owning editor. No document snapshots are copied into panes. Shared drawing and input geometry keeps the focused viewport, chooser buttons, and pane headers aligned.

Regression tests exercise nested splits, independent camera state, shared edits/undo, switching between distinct document owners, and preservation of the text rendering transform when the prefix overlay is absent. Empty overlays are skipped because Vello 0.10 clears pending transform/style resets when appending an empty scene, which otherwise offsets the Tools panel. Run `cargo test -p amalith-shell multiplexer --lib`. The opt-in GPU review (`cargo test -p amalith-shell render_pane_review --lib -- --ignored`) writes `/tmp/amalith-multiplexer-review.png` with two views of one document and a chooser.

Full checks: `cargo build --workspace --tests --examples` and `cargo test --workspace`. Native keyboard, mouse, and interactive shell behavior also require an application walkthrough.
