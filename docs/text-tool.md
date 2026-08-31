# Text tool + Character panel — research & plan

Status: **research only, nothing implemented.** Snapshot before this work: commit
`0352c1c`.

The goal: a Type (T) tool that does Illustrator-style **point type** (click, type,
auto-grows) and **area/paragraph type** (drag a box, text wraps), plus a
**Character panel** wired to the real type attributes.

---

## 1. How Illustrator's Type tool actually works

### Activation
- Toolbar Type tool, or press **T**. Cursor becomes an I-beam inside a dotted
  square (area) / plain I-beam (point), with a small box glyph when hovering an
  existing text object.

### Point type
- **Click** on empty canvas → a blinking caret appears at that point; start
  typing. The object has **no width constraint** — it grows to fit; lines only
  break where you press **Return**.
- The click point is the **first baseline, left edge** (for left-aligned text).
- Its bounding box hugs the text and moves/resizes with the content.

### Area (paragraph) type
- **Drag** a rectangle → a fixed-size container. Text **wraps** to the box
  width; when it doesn't fit, an **overflow indicator** (a red **+** in a small
  box on the lower-right out-port) appears.
- The drag rect's **top-left** is the anchor; first baseline sits one leading
  down from the top (roughly).
- Resizing the box (Selection tool handles) re-wraps; it does **not** scale the
  glyphs. Height can be fixed or "auto" (auto grows downward).

### Entering / leaving edit mode
- With the **Type tool**: click into any text object to edit; the caret lands
  at the clicked character.
- With the **Selection tool**: **double-click** a text object → drops into edit
  mode (temporarily acts as the Type tool).
- Commit/exit: **Esc**, **Cmd-click** elsewhere, switch tools, or Cmd+Return.
- An **empty** text object (nothing typed) is **discarded** on commit.
- Modern IL seeds new type with greyed **placeholder** ("Lorem ipsum…") that
  vanishes on first keystroke — optional; we can skip it.

### Selection semantics
- **Type tool** in a text object: character caret + range selection
  (click, shift-click, drag, double-click = word, triple/quadruple = line/para,
  Cmd+A = all).
- **Selection tool**: selects the whole text object (bounding box + transform
  handles) — move / scale / rotate the object, not the text.
- **Direct Selection**: edits the text *path* for area type / the baseline path
  for type-on-a-path (out of scope for v1).

### Out of scope for v1
Type on a path, text threading between boxes, text wrap around objects,
vertical text, tab stops / tabs panel, OpenType panel, glyphs panel,
find/replace, spell-check.

---

## 2. Text stack we already have

`crates/amalith-shell/src/text.rs` wraps **parley 0.11** (`FontContext` +
`LayoutContext`) and **vello** glyph drawing. It currently only lays out
single lines / wrapped blocks for *chrome* (`draw`, `measure`, `wrap`,
`draw_layout`). No editing, no caret, no IME.

`Rename` (the panel inline-rename) is the only "text input" — a bare `String`
with append + backspace, **no caret, no selection, no IME**. Not reusable for a
real editor.

### parley gives us a real editor for free: `parley::PlainEditor<Brush>`
(`parley::editing`). Key surface:

| need | API |
|---|---|
| create | `PlainEditor::new(font_size)` |
| set/get text | `set_text(&str)`, `text() -> SplitString` (splices in IME preedit), `raw_text()` |
| point vs area | `set_width(None)` = point (auto), `set_width(Some(px))` = area (wrap) |
| alignment | `set_alignment(Alignment::{Start,Middle,End,Justified})` |
| DPI | `set_scale(f32)` |
| whole-object style | `edit_styles() -> &mut StyleSet<Brush>` — push `StyleProperty` |
| layout for drawing | `layout(font_cx, layout_cx) -> &Layout<Brush>` / `try_layout()` |
| all edits/nav | `driver(font_cx, layout_cx) -> PlainEditorDriver` |
| driver: typing | `insert_or_replace_selection`, `delete`, `delete_word`, `backdelete`, `backdelete_word` |
| driver: nav | `move_left/right/up/down`, `move_word_left/right`, `move_to_line_start/end`, `move_to_text_start/end`, `move_to_point(x,y)`, `move_to_byte` |
| driver: select | `select_*` mirrors of all the above, `select_all`, `collapse_selection`, `select_word_at_point`, `select_line_at_point`, `extend_selection_to_point` |
| draw helpers | `selection_geometry() -> Vec<(BoundingBox, usize)>`, `cursor_geometry(size) -> Option<BoundingBox>` |
| IME | `set_compose(text, cursor)`, `set_compose_byte_range`, `clear_compose`, `finish_compose`, `is_composing`, `ime_cursor_area()` |

**Big limitation:** `PlainEditor` is **one style for the whole buffer**
(`default_style: StyleSet`). Illustrator's Character panel styles a *selected
range*. So v1 = "the Character panel edits the whole text object". Per-character
runs (select 3 letters, bold them) is v2 and needs a custom editor built over
`parley::Layout` + our own runs model + a styled builder. **Call this out to the
user before building — it's the one real divergence from IL.**

### Fonts
`parley::FontContext.collection` is public (`fontique::Collection`):
- `collection.family_names() -> impl Iterator<Item=&str>` — the whole installed
  list for the family dropdown.
- `collection.family_by_name(name) -> Option<FamilyInfo>`, then
  `FamilyInfo::fonts() -> &[FontInfo]` for the **style** dropdown
  (each `FontInfo` carries weight / style / width → label as
  "Regular / Bold / Italic / Light / Condensed …").

System fonts only. No embedded-font management, no font preview in the menu,
no "recently used" for v1.

---

## 3. Model changes (`amalith-core`)

`ObjectKind::Text(TextData)` exists but is a stub: `{ content: String,
local_bounds: Rect }`. Rework:

```rust
pub struct TextData {
    pub content: String,
    pub kind: TextKind,
    pub style: TextStyle,          // v1: whole-object
    pub align: TextAlign,          // Start | Center | End | Justify
    pub local_bounds: Rect,        // recomputed from layout after every edit
}

pub enum TextKind {
    Point,                         // anchor = first-baseline left; auto width
    Area { width: f64, height: Option<f64> },  // None height = auto-grow
}

pub struct TextStyle {
    pub family: String,            // portable: family name, not a font handle
    pub weight: u16,               // 100..900
    pub italic: bool,
    pub width: FontWidthClass,     // normal / condensed / expanded (v2-ish)
    pub size_px: f64,
    pub leading: Leading,          // Auto (≈1.2×) | Absolute(px)
    pub tracking: f64,             // IL "thousandths of an em"; → letter-spacing px
    pub underline: bool,
    pub strikethrough: bool,
    pub caps: Caps,                // None | All | Small   (v1: None|Small via `smcp`)
    pub position: TextPosition,    // Normal | Super | Sub  (via `sups`/`subs`)
    // v2: h_scale, v_scale, baseline_shift, char_rotation, kerning mode
}
```

Serialize by **family name + weight + italic** (portable across machines; on
load, resolve against `fontique`, fall back to a default if missing).

### New commands (`amalith-commands`) + `Edit` variants
- `Command::CreateText { layer, kind, origin, style }` → `Edit::InsertObject`
- `Command::SetTextContent { object, content }` → new `Edit::SetTextContent`
- `Command::SetTextStyle { object, style }` (or granular per-attribute) → `Edit::SetTextStyle`
- `Command::SetTextBox { object, width, height }` (area resize) → reuse/extend

Bounds: after any content/style/box change, re-lay-out and write
`TextData.local_bounds` (needed by selection, snapping, Layers bounds).
This means the **command layer needs a parley layout pass**, or the shell
recomputes bounds and passes them into the command. Simpler: shell computes,
command just stores (like `CreateArtboard` already takes an explicit rect).

---

## 4. Editor integration (`amalith-shell`)

### Tool
- `Tool::Text` (key **T**), `Icon::Text`, add to `Tool::ALL`, `is_shape()` stays
  false, add a tools-panel slot + a branding SVG glyph.

### App state
```rust
struct TextEdit {
    object: ObjectId,
    editor: parley::PlainEditor<vello::peniko::Brush>,
    kind: TextKind,
    origin: amalith_core::Point,   // doc-space anchor
    dirty: bool,                   // needs commit
    blink: std::time::Instant,     // caret phase
}
// App { …, text_edit: Option<TextEdit> }
```

### Interaction (in `on_press` / `on_cursor_move` / `on_release`, gated on `Tool::Text`)
- **click empty canvas** → `Command::CreateText { kind: Point, origin: docPoint }`,
  build a `PlainEditor` (`set_width(None)`, `set_scale`, seed styles from the
  "new text defaults"), enter edit, show caret.
- **drag** → rubber-band a box (reuse the shape-tool rubber-band); on release
  `CreateText { kind: Area { width, height: None } }`, `editor.set_width(Some(w))`.
- **click on existing Text** (Text tool) → enter edit, `driver.move_to_point`.
- **Selection tool + double-click on Text** → set `active_tool = Text`, enter edit.

### Edit-mode input
- **Keyboard** (`WindowEvent::KeyboardInput`, when `text_edit.is_some()` — check
  *before* the canvas/tool key handling, like `newdoc`/`rename` already do):
  - arrows / Home / End / ⌥+arrows (word) / ⌘+arrows (line/doc) — with Shift =
    the `select_*` variants
  - Backspace / Delete / ⌥+Backspace (word) / ⌘+Backspace (line)
  - **Return** → `insert_or_replace_selection("\n")`; **Tab** → literal `\t`
  - **⌘A** select all; **⌘C/X/V** → `selected_text()` / `insert_or_replace_selection`
    through `arboard` (plain text — the OS SVG-clipboard bridge is bypassed here)
  - **Esc** → cancel/commit; **⌘Return** → commit
  - **⌘Z while editing**: v1 = commit the object, then normal doc undo (coarse).
    v2 = per-edit undo ring inside `TextEdit`.
- **IME** (`WindowEvent::Ime`): on enter-edit `window.set_ime_allowed(true)`;
  each frame `window.set_ime_cursor_area(editor.ime_cursor_area())`;
  `Ime::Preedit(s, cur)` → `driver.set_compose(&s, cur)`;
  `Ime::Commit(s)` → `driver.clear_compose(); insert_or_replace_selection(&s)`;
  `Ime::Disabled` → `driver.clear_compose()`.
- **Pointer**: down → `driver.move_to_point`; drag → `extend_selection_to_point`;
  dbl → `select_word_at_point`; triple → `select_line_at_point`.
  Convert screen → editor space: `p_editor = (p_screen - text_origin_screen) / view.zoom`
  (keep the editor at font px; apply `view.zoom` only as a draw transform +
  pointer inverse). `set_scale(self.scale)` handles DPI only.

### Commit (`commit_text_edit`)
`editor.text()` → if empty, `Command::DeleteObject`; else
`Command::SetTextContent` + `SetTextStyle` + recomputed bounds; drop `text_edit`;
`window.set_ime_allowed(false)`.

### Rendering (`canvas.rs`)
- `ObjectKind::Text` in `paint_object` (currently just a grey box):
  - lay out with the object's style (cache a `parley::Layout` on the shell keyed
    by `(content, style, width, zoom*scale)` — or a per-object cache map);
    draw glyph runs with the existing `text.rs` glyph loop; honour the object
    transform + `view` transform; draw underline/strike from parley decorations.
- If `text_edit.object == id`: draw the **live** `editor.layout()` instead, plus
  - selection rects from `selection_geometry()` (theme select-blue, ~40% alpha)
  - blinking caret from `cursor_geometry(1.0)` (toggle on `blink`)
  - for area type: the box outline + the red **+** overflow marker when
    `layout.height() > box_height`
- `update_canvas_cursor`: I-beam while Text tool over canvas; suppress the
  Space node-peek and tool glyphs while `text_edit.is_some()`.

---

## 5. The Character panel

New panel, same pattern as `layers`/`swatches`: `panels/character.rs` with
`paint(scene, text, body, ctx)` + `hit(body, local, ctx) -> Action`; add the
`"character"` arm to `panels/mod.rs`; register; add to the right rail in the
default dock (`demo_right_dock`).

### Source of truth
- If `text_edit.is_some()` → that editor's style.
- Else if a single `Text` object is selected → its `style`.
- Else → **"new text defaults"** stored on `App` (what the next Create uses).
- Mixed multi-selection → show blanks / "—".

### Controls (screenshot order) and where each maps

| Panel control (IL glyph) | Value | v1 mapping | v1? |
|---|---|---|---|
| Font family — `Myriad Pro` | combobox | `family` → `StyleProperty::FontFamily`; list from `collection.family_names()` | ✅ |
| Font style — `Regular` | combobox | `weight`+`italic`(+`width`) → `FontWeight`/`FontStyle`; options from `FamilyInfo::fonts()` | ✅ |
| Font size `T` — `12 pt` | stepper+field+menu | `size_px` → `FontSize` | ✅ |
| Leading `↕A` — `(14.4 pt)` | stepper+field | `leading`: Auto = `LineHeight::FontSizeRelative(1.2)`, value = `LineHeight::Absolute(px)` | ✅ |
| Kerning `V/A` — `Auto` | menu (Auto/Optical/Metrics/num) | parley has no kerning modes. Metrics/Auto = default (font `kern` on). Optical = unsupported (show, treat as Auto). Numeric = fold into tracking. | ⚠️ partial |
| Tracking `VA` — `0` | stepper+field | `tracking` (‰ em) → `StyleProperty::LetterSpacing(tracking/1000 * size_px)` | ✅ |
| Vertical scale `↕T` — `100%` | stepper+field | geometric — needs a non-uniform scale on the text object transform + bounds plumbing | ❌ v2 |
| Horizontal scale `T→` — `100%` | stepper+field | same as above | ❌ v2 |
| Baseline shift `A↑a` — `0 pt` | stepper+field | per-run in IL; whole-object = a y offset of the anchor. Marginal value whole-object. | ❌ v2 |
| Char rotation `(T)` — `0°` | stepper+field | per-glyph rotation; parley: none | ❌ v2 |
| All Caps `TT` | toggle | transform displayed string (keep original in model) or OT `case` | ⚠️ v1.5 |
| Small Caps `Tr` | toggle | `FontFeatures("smcp")` (font-dependent) | ✅ |
| Superscript `T¹` | toggle | `FontFeatures("sups")` | ✅ |
| Subscript `T₁` | toggle | `FontFeatures("subs")` | ✅ |
| Underline `T̲` | toggle | `StyleProperty::Underline(true)` | ✅ |
| Strikethrough `T̶` | toggle | `StyleProperty::Strikethrough(true)` | ✅ |

### New `Action` variants
`SetFontFamily(String)`, `SetFontFace { weight: u16, italic: bool }`,
`SetFontSize(f64)`, `SetLeading(Leading)`, `SetTracking(f64)`,
`SetKerning(KerningMode)`, `ToggleTextFlag(TextFlag)` (AllCaps / SmallCaps /
Super / Sub / Underline / Strike). App applies to the live editor's
`edit_styles()` (and `set_alignment` / `set_line_height`) *and* to the
selected object's `style` via `Command::SetTextStyle`, or to "new text
defaults".

### Missing widget infrastructure — this is ~half the panel work
The shell has **no** real text field or combobox. Need:
1. **Numeric field**: editable value + up/down steppers + a preset dropdown
   (`12 / 14 / 18 / 24 / 36 / 48 / 60 / 72 / …` for size, etc.). Needs a caret,
   selection, click-to-place, drag-select, ⌘A, arrow-key nudge (±1, Shift ±10),
   scrub-drag on the label. Cleanest: a tiny `parley::PlainEditor` per field
   (same machinery as the canvas editor).
3. **Combobox**: text-filterable list (font family — hundreds of entries →
   needs type-ahead + scroll), plain list (font style). Reuse the numeric
   field's editor for the filter box + a scrolling popup list (the colour
   `picker` overlay is a precedent for a floating popup).

Budget the widget kit as its own phase.

---

## 6. Phasing

1. **Type tool + editing loop.** `Tool::Text`; point + area creation;
   `PlainEditor` keyboard + pointer + IME; commit/cancel; canvas renders
   committed text and the live-edit overlay (caret, selection, box, overflow).
   One hardcoded default style. **No panel.**
2. **Widget kit.** Reusable numeric field + combobox (built on `PlainEditor`).
3. **Character panel v1.** family / style / size / leading / tracking /
   underline / strike wired to the whole-object style + "new text defaults";
   font enumeration from `fontique`.
4. **OT + polish.** small caps, super/sub via `FontFeatures`; area auto-height;
   edit-from-Selection double-click; plain-text copy/paste; caret blink; in-edit
   undo ring.
5. **v2.** per-character runs (custom layout editor); H/V scale, baseline shift,
   char rotation; optical kerning; all-caps; type-on-a-path.

---

## 7. Decisions to get from Tony before coding

1. **Single-style v1** (Character panel edits the whole text object, not a
   selected range) — acceptable, or is per-character styling a must-have for v1?
   (Per-char is a large multiplier.)
2. **In-edit undo**: fine for v1 that ⌘Z during editing commits the object then
   undoes the whole thing (coarse), refine later?
3. **Placeholder text** ("Lorem ipsum") on new type — want it or not?
4. **Area auto-height** default (grow down as you type) vs fixed drag height?
5. Which of the ❌/⚠️ panel controls must be live for the first usable version
   vs shown-but-disabled.
