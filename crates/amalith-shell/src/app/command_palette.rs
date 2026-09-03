//! The ⌘K command palette — building its entry list from the menu
//! actions / preferences / tools, and running the chosen entry. The
//! palette widget itself is [`crate::palette`]; this is the `App` glue.
//! Split out of `app/mod.rs`.

use super::*;

impl App {
    /// Open the command palette, rebuilding its command list from the
    /// current state so toggle labels ("Hide/Show Guides") are right.
    pub(in crate::app) fn open_palette(&mut self) {
        if self.palette.is_some() {
            self.palette = None;
            self.request_main_redraw();
            return;
        }
        self.ctx_menu = None;
        self.ruler_menu = None;
        self.panel_menu = None;
        let (entries, kinds) = self.build_palette_commands();
        self.palette_kinds = kinds;
        self.palette = Some(crate::palette::Palette::new(entries));
        self.request_main_redraw();
    }

    /// `(display entries, parallel actions)` for the palette.
    fn build_palette_commands(&self) -> (Vec<crate::palette::Entry>, Vec<PaletteKind>) {
        use crate::palette::Entry;
        let mut e: Vec<Entry> = Vec::new();
        let mut k: Vec<PaletteKind> = Vec::new();
        let mut add = |title: String, hint: &str, kind: PaletteKind| {
            e.push(Entry { title, hint: hint.to_string() });
            k.push(kind);
        };

        for &t in &Tool::ALL {
            add(format!("{} Tool", t.label()), "Tool", PaletteKind::Tool(t));
        }

        let menu: &[(&str, &str, MenuAction)] = &[
            ("New", "File", MenuAction::New),
            ("Open…", "File", MenuAction::Open),
            ("Save", "File", MenuAction::Save),
            ("Save As…", "File", MenuAction::SaveAs),
            ("Import SVG…", "File", MenuAction::ImportSvg),
            ("Place…", "File", MenuAction::Place),
            ("Add Scripts Folder…", "File", MenuAction::AddScriptsFolder),
            ("Undo", "Edit", MenuAction::Undo),
            ("Redo", "Edit", MenuAction::Redo),
            ("Cut", "Edit", MenuAction::Cut),
            ("Copy", "Edit", MenuAction::Copy),
            ("Paste", "Edit", MenuAction::Paste),
            ("Duplicate", "Edit", MenuAction::Duplicate),
            ("Select All", "Edit", MenuAction::SelectAll),
            ("Bring Forward", "Arrange", MenuAction::BringForward),
            ("Bring to Front", "Arrange", MenuAction::BringToFront),
            ("Send Backward", "Arrange", MenuAction::SendBackward),
            ("Send to Back", "Arrange", MenuAction::SendToBack),
            ("Zoom In", "View", MenuAction::ZoomIn),
            ("Zoom Out", "View", MenuAction::ZoomOut),
            ("Fit Artboard in Window", "View", MenuAction::FitArtboard),
            ("Fit All in Window", "View", MenuAction::FitAll),
            ("Clear Guides", "View", MenuAction::ClearGuides),
            ("Preferences…", "App", MenuAction::Preferences),
            ("About Amalith", "App", MenuAction::About),
        ];
        for (title, hint, a) in menu {
            add(title.to_string(), hint, PaletteKind::Menu(a.clone()));
        }

        // Stateful toggles — label reflects what the command will do.
        add(
            if self.outline_mode { "Exit Outline View" } else { "Outline View" }.to_string(),
            "View",
            PaletteKind::Menu(MenuAction::ToggleOutline),
        );
        add(
            if self.guides_hidden { "Show Guides" } else { "Hide Guides" }.to_string(),
            "View",
            PaletteKind::Menu(MenuAction::ToggleGuides),
        );
        add(
            if self.guides_locked { "Unlock Guides" } else { "Lock Guides" }.to_string(),
            "View",
            PaletteKind::Menu(MenuAction::ToggleGuideLock),
        );

        for (id, label) in WINDOW_PANELS {
            let on = self.dock.contains(PanelId(id));
            add(
                format!("{} {} Panel", if on { "Hide" } else { "Show" }, label),
                "Panel",
                PaletteKind::Menu(MenuAction::TogglePanel(id)),
            );
        }

        for (i, name) in prefs::CATEGORIES.iter().enumerate() {
            add(format!("Preferences: {name}"), "App", PaletteKind::Prefs(i));
        }

        (e, k)
    }

    /// Run the palette row at `idx` (its original entry index) and close.
    pub(in crate::app) fn run_palette_cmd(&mut self, idx: usize) {
        let kind = self.palette_kinds.get(idx).cloned();
        self.palette = None;
        match kind {
            Some(PaletteKind::Menu(a)) => self.run_menu_action(a),
            Some(PaletteKind::Tool(t)) => self.set_tool(t),
            Some(PaletteKind::Prefs(cat)) => {
                self.prefs = Some(prefs::Prefs::new(
                    self.settings,
                    self.scripts.clone(),
                    self.keymaps.clone(),
                ));
                if let Some(p) = &mut self.prefs {
                    p.category = cat.min(prefs::CATEGORIES.len() - 1);
                }
            }
            None => {}
        }
        self.request_main_redraw();
    }
}
