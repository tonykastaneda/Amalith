# `Tool::ALL` / `PrefAction::ALL` — hand-maintained arrays with no compiler tie to their enum

**Complexity: Easy** — a single test addition, no behavior change, no dependencies on other docs.

## The problem

This is the pattern first found firsthand while building the Free Transform
tool: adding one new `Tool` variant
(`crates/amalith-shell/src/tool.rs`) required touching all of:

- `tool.rs`'s `Tool::ALL` array, plus the `label()`/`key()`/`icon()` match
  arms
- `prefs.rs`'s `default_tool_key()` exhaustive match
- `icons.rs`'s `Icon` enum plus its `brand_svg()`/`draw()` match arms
- `panels/tools.rs`'s `slots()` array, plus `natural_height()`'s hardcoded
  row-count divisor
- `settings.rs`'s `tool_name()` match

Six separate places for "register one new tool." Most of these *are*
compiler-enforced (anything matching directly over the `Tool` enum forces an
arm), which is why every one of them got caught at compile time during the
Free Transform build — but `Tool::ALL` itself is a hand-written fixed-size
array (`pub const ALL: [Tool; 24] = [...]`), and nothing ties its contents to
the enum's actual variant list. Forget to add a variant to `ALL` and the
build still succeeds; the new tool just has no default keyboard shortcut, no
toolbar slot, and silently never appears in `Tool::ALL`-indexed arrays like
`Settings::tool_keys`.

`PrefAction` (`crates/amalith-shell/src/prefs.rs:178`) is the same shape,
smaller blast radius: `PrefAction::ALL` (line 192) is a hand-maintained
array with no compiler tie to the enum. Missing a variant there means no
default keybinding, no entry in the Preferences ▸ Keyboard page, and no
persistence round-trip — while `run_pref_action`'s actual dispatch (which
*does* match over the enum) stays fully compiler-enforced.

## Why it matters

Lower severity than the `PanelId` case (`13-panel-registry-refactor-hard.md`)
because most of the wiring per-variant *is* enforced — but the one place
that isn't (`ALL`) is also the one place a forgotten entry is hardest to
notice: the tool/action still exists, still runs, just quietly has no
shortcut and doesn't show up in the places that iterate `ALL`.

## What needs to happen

1. Add a compile-time or test-time check that `Tool::ALL.len() ==
   Tool`'s variant count, and similarly that every `Tool` variant appears
   exactly once in `ALL`. Since Rust doesn't have reflection to enumerate
   enum variants automatically without a derive macro or codegen, the
   pragmatic fix is a test (not full compiler enforcement):
   ```rust
   #[test]
   fn tool_all_covers_every_variant() {
       // for each variant, assert it appears in Tool::ALL exactly once —
       // exhaustive match with a compile error as the enforcement, e.g.
       // a match that does nothing but return the variant, forcing this
       // test itself to fail to compile if a variant is missing from the
       // match arms used to build the assertion list.
   }
   ```
2. Apply the same test pattern to `PrefAction::ALL`.
3. Longer-term alternative: a proc-macro derive (`#[derive(AllVariants)]`)
   that generates the `ALL` array from the enum definition directly, so this
   whole class of drift becomes structurally impossible rather than
   test-caught. Only worth the investment if more `X::ALL`-shaped enums show
   up elsewhere.

## How it needs to happen

- This is small and low-risk — a single test file addition, no behavior
  change. Good candidate to do early and independently of the larger
  refactors in this folder.
