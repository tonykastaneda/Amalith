# CONTEXT — for anyone (human or agent) working on Amalith

Amalith is an Illustrator-class vector editor in Rust: `amalith-core` (document
model) → `amalith-commands` (the one undoable mutation path) → `amalith-io`
(`.amalith`/SVG) → `amalith-shell` (winit + wgpu + vello GUI, no widget
toolkit). **The GUI never mutates the document directly — everything goes
through `Editor::execute`.** See `README.md` for the crate breakdown and
`amalith-project-brief.md` for the why.

## What to run

| You want… | Command |
|---|---|
| **"run it" / "open the app" / dev loop** | `cargo run -p amalith-shell` |
| tests | `cargo test --workspace` |
| **"build" / "package" / "cut a build"** | `./scripts/package.sh` |
| just a local signed macOS `.app` | `./scripts/package-macos.sh` |

"run" and "build" mean different things here — **"run" is `cargo run`**, **"build"
is the packaging script.**

## `./scripts/package.sh`

Produces, in `dist/`:

- `dist/Amalith.dmg` — macOS. Signed with Developer ID + hardened runtime when
  the env vars below are set; otherwise an unsigned `.app` + `.dmg`.
- `dist/Windows/` — `Amalith.exe` (self-contained, static CRT — nothing to
  install), its `.ico`, and a `README.txt`. Cross-compiled from macOS.

Default run = fast (signs the `.app`, skips Apple notarization). For a real
public release, notarize:

```bash
SIGN_IDENTITY="Developer ID Application: Anthony Castaneda (GM98GV6S97)" \
NOTARY_PROFILE=amalith \
  ./scripts/package.sh
```

The notary credentials live in the login keychain under the profile name
`amalith` (`xcrun notarytool store-credentials`). Team ID `GM98GV6S97`.
`package.sh` notarizes and staples both the `.app` and the `.dmg`.

Windows signing is **not** set up and is **not required** — an unsigned exe
runs; first launch just shows a SmartScreen "unknown publisher" notice the
user clicks past.

## Toolchain notes (macOS build host)

- This Mac has **two Rust installs**: a Homebrew `rust` formula (what bare
  `cargo` resolves to — fine for the native mac build) and `rustup` (needed for
  cross-compiling, since Homebrew rust can't `rustup target add`).
  `package.sh` forces the rustup toolchain for the Windows step via
  `rustup which cargo`.
- Windows cross-compile needs, one-time:
  `rustup target add x86_64-pc-windows-msvc`, `cargo install cargo-xwin`,
  `brew install llvm` (for `llvm-rc`, used by `build.rs` to embed the icon).
- If a link step ever dies with `lld-link ... SIGABRT` and
  `Library not loaded: @rpath/libLLVM.dylib`, the rustup toolchain is missing
  its LLVM tools: `rustup component add llvm-tools`.
- `cargo-xwin` downloads a ~1 GB MS SDK/CRT into `~/Library/Caches/cargo-xwin`
  on first use. The `LNK4099` "cannot use debug info for libcmt.lib" warnings
  it prints are harmless (Microsoft never ships those PDBs).

## Where the deeper context lives

- `README.md` — crates, architecture invariant, website.
- `docs/text-tool.md` — the text engine design.
- `PERFORMANCE.md` — the rendering perf pass. Also load-bearing since:
  rendering is **on-demand** (a new feature that changes the screen without
  calling `App::request_main_redraw` silently won't repaint), and glyph
  drawing must stay `.hint(false)` (hinting re-runs per frame and tanks fps
  while the canvas redraws).
- `.claude/` `memory/` — Claude's running notes on this codebase (feature
  wiring, gotchas). Non-Claude agents can skim these too; they're plain
  markdown.
