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
| **"build" / "build a new version" / "release"** | GitHub CI — see below. Never build release artifacts locally. |

"run" and "build" mean different things here — **"run" is `cargo run`**,
**"build" is a GitHub release built entirely in CI.**

## Building a new version (CI only)

There is no local packaging path. Releases are built, signed, notarized and
published by `.github/workflows/release.yml`; nothing is built on a
developer machine. "Build new version" means:

1. Bump `version` under `[workspace.package]` in the root `Cargo.toml`
   (patch bump unless told otherwise) and refresh `Cargo.lock`.
2. Commit and push to `main`. `build.yml` compiles and tests on every
   platform — wait for it to go green.
3. Tag `vX.Y.Z` on that commit and push the tag. `release.yml` builds all
   three platforms and publishes the GitHub Release (full release, marked
   Latest). CI uses the tag as the version, so tag and binaries always match.
4. Check the published release has exactly these three assets:
   - `Amalith.dmg` — macOS arm64, Developer ID signed + notarized
   - `Amalith-Setup.exe` — Windows installer (Inno Setup, unsigned;
     SmartScreen warns). Installs `Amalith.exe` + `Amalith.com` (console
     front door for `Amalith script`), shortcuts, `.amalith` association,
     optional PATH entry. CI installs it, runs a script through it and
     uninstalls it before publishing.
   - `Amalith-Linux.zip` — AppImage, deb, rpm, tarball, Arch PKGBUILD,
     `INSTALL.txt`, `SHA256SUMS`

5. Website: the Downloads buttons fetch the newest release from the GitHub
   API at page load, so a new version needs no site edit. Only touch
   `website/` if the release changed something the site describes (new
   platform, renamed asset, a feature or install step the docs mention).
   Pushing `website/**` redeploys via `pages.yml`.

The website matches those three filenames, so don't rename them. Signing credentials live in repo secrets (see the header of
`release.yml`). The `scripts/package-*` files are the per-platform steps CI
runs; they aren't meant to be run by hand.

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
