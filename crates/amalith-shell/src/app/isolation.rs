//! Isolation mode and clipping masks — the breadcrumb stack, entering /
//! leaving isolation, and making / releasing a clip group. The scoped
//! hit-testing lives in [`crate::select`]; the scrim + breadcrumb bar are
//! painted in `app/render`. Split out of `app/mod.rs`.

use super::*;

impl App {
    /// The group the canvas is currently scoped into, if any.
    pub(in crate::app) fn isolation_root(&self) -> Option<ObjectId> {
        self.isolation.last().copied()
    }

    /// The extra transform needed to convert between document space and
    /// the current isolation's own local space — identity unless isolated
    /// *into a symbol instance specifically*. A plain group's own world
    /// transform is already correct via the normal parent chain (there's
    /// only one of it), but a symbol definition has none of its own by
    /// design (`Document::world_transform` treats `ObjectParent::Symbol`
    /// as identity, same as a bare layer, since the same definition can
    /// sit under any number of differently-placed instances) — so every
    /// child's stored geometry is really in "definition-local" space, and
    /// needs this instance's own real world transform to mean anything on
    /// screen. `select::topmost_in`'s doc comment has the full reasoning;
    /// this is the same correction, exposed for every other isolation-
    /// scoped hit-test/render call site to share (anchor/handle hit-
    /// testing in `anchors.rs`, selection-box rendering in `canvas.rs`)
    /// instead of re-deriving it ad hoc.
    pub(in crate::app) fn isolation_ambient(&self) -> Affine {
        let Some(root) = self.isolation_root() else { return Affine::IDENTITY };
        let doc = self.doc.editor.document();
        if matches!(doc.object(root).map(|o| &o.kind), Some(amalith_core::ObjectKind::Symbol(_))) {
            convert::affine(doc.world_transform(root))
        } else {
            Affine::IDENTITY
        }
    }

    /// Whether any level of the current isolation breadcrumb is a Symbol
    /// instance (Edit Symbol) rather than a plain group — checked at
    /// every depth, not just the deepest, since drilling into an ordinary
    /// group *inside* a symbol's content is still editing that symbol's
    /// shared definition. Drives the isolation bar's purple underline
    /// (`paint_isolation_bar`) so editing a symbol always reads as
    /// visually distinct from ordinary group isolation.
    pub(in crate::app) fn is_editing_symbol(&self) -> bool {
        let doc = self.doc.editor.document();
        self.isolation.iter().any(|&id| {
            matches!(doc.object(id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Symbol(_)))
        })
    }

    /// Every "draw a brand-new object" path (shape tools, Pen, Type,
    /// placed images, …) creates its object on the active layer via
    /// `ensure_layer()` — that's the only parent a `Create*` command
    /// knows how to target. If the canvas is currently isolated, a freshly
    /// drawn object has to be moved into whatever's isolated instead, or
    /// isolation would be purely cosmetic for drawing: the new shape
    /// would just sit on the layer, invisible to the group/symbol you're
    /// actually working inside and never shared by other instances.
    /// Called right after every such creation succeeds, a no-op when
    /// nothing is isolated.
    ///
    /// A plain group's own world transform is already well-defined (there
    /// is only one of it), so `Command::Reparent` rebases onto it
    /// correctly on its own. A symbol *definition* has no such thing — by
    /// design its content has no fixed position, only whatever transform
    /// each instance places on it (`Document::world_transform` returns
    /// identity for `ObjectParent::Symbol`, same as a Layer) — so
    /// reparenting into one always keeps the object's *absolute* document
    /// transform verbatim. Since the object was drawn to look right on
    /// screen through *this specific instance*, it has to be rebased onto
    /// *that instance's* world transform by hand first (`SetTransform`),
    /// not the (nonexistent) definition's — otherwise it renders in the
    /// wrong place through every instance, including the one just edited.
    pub(in crate::app) fn reparent_new_object_into_isolation(&mut self, id: ObjectId) {
        let Some(root) = self.isolation_root() else { return };
        let doc = self.doc.editor.document();
        let kind = doc.object(root).map(|o| &o.kind);
        match kind {
            Some(amalith_core::ObjectKind::Group(_)) => {
                let _ = self.doc.editor.execute(Command::Reparent {
                    ids: vec![id],
                    parent: amalith_core::ObjectParent::Group(root),
                    index: usize::MAX,
                });
            }
            Some(amalith_core::ObjectKind::Symbol(data)) => {
                let definition = data.definition;
                let instance_world = doc.world_transform(root);
                let Some(current) = doc.object(id).map(|o| o.transform) else { return };
                let rebased = instance_world.inverse() * current;
                let _ = self.doc.editor.execute(Command::SetTransform { object: id, transform: rebased });
                let _ = self.doc.editor.execute(Command::Reparent {
                    ids: vec![id],
                    parent: amalith_core::ObjectParent::Symbol(definition),
                    index: usize::MAX,
                });
            }
            _ => return,
        }
        self.request_main_redraw();
    }

    /// Drop breadcrumb entries whose object no longer exists. Every level
    /// except the deepest must still be a group or a symbol instance (Edit
    /// Symbol) to drill through; the deepest may be a bare object (path /
    /// shape / image).
    pub(in crate::app) fn prune_isolation(&mut self) {
        let doc = self.doc.editor.document();
        let n = self.isolation.len();
        let good = self
            .isolation
            .iter()
            .enumerate()
            .take_while(|(i, id)| match doc.object(**id).map(|o| &o.kind) {
                None => false,
                Some(amalith_core::ObjectKind::Group(_)) => true,
                Some(amalith_core::ObjectKind::Symbol(_)) => true,
                Some(_) => *i + 1 == n,
            })
            .count();
        if good != self.isolation.len() {
            self.isolation.truncate(good);
        }
    }

    /// Enter (or drill deeper into) isolation on `id`. Any object except
    /// text can be isolated: a group opens its contents, a symbol instance
    /// opens its *definition's* contents (Edit Symbol — every other
    /// instance keeps showing the unedited version, exactly like every
    /// instance keeps working after `Command::BreakSymbolLink` on a
    /// different one), and a bare path, shape or image just dims
    /// everything else and scopes selection to itself.
    pub(in crate::app) fn enter_isolation(&mut self, id: ObjectId) {
        let is_container = match self.doc.editor.document().object(id).map(|o| &o.kind) {
            Some(amalith_core::ObjectKind::Text(_)) | None => return,
            Some(amalith_core::ObjectKind::Group(_) | amalith_core::ObjectKind::Symbol(_)) => true,
            Some(_) => false,
        };
        if self.isolation.last() == Some(&id) {
            return;
        }
        self.isolation.push(id);
        self.doc.selection = if is_container { Vec::new() } else { vec![id] };
        self.sync_align_mode();
        self.request_main_redraw();
    }

    /// Step out one breadcrumb level.
    pub(in crate::app) fn pop_isolation(&mut self) {
        if let Some(id) = self.isolation.pop() {
            self.doc.selection = if self.doc.editor.document().object(id).is_some() {
                vec![id]
            } else {
                Vec::new()
            };
            self.request_main_redraw();
        }
    }

    /// Truncate the breadcrumb to `depth` groups (0 = fully exit).
    pub(in crate::app) fn isolation_to_depth(&mut self, depth: usize) {
        if depth >= self.isolation.len() {
            return;
        }
        let keep = self.isolation.get(depth.wrapping_sub(1)).copied();
        self.isolation.truncate(depth);
        self.doc.selection = match keep {
            Some(id) if self.doc.editor.document().object(id).is_some() => vec![id],
            _ => Vec::new(),
        };
        self.request_main_redraw();
    }

    /// Breadcrumb labels: the owning layer, then each isolated group.
    pub(in crate::app) fn isolation_crumbs(&self) -> Vec<String> {
        if self.isolation.is_empty() {
            return Vec::new();
        }
        let doc = self.doc.editor.document();
        let mut out = Vec::new();
        // Owning layer of the outermost isolated group.
        let mut walk = self.isolation[0];
        let layer_name = loop {
            match doc.object(walk).map(|o| o.parent) {
                Some(amalith_core::ObjectParent::Layer(l)) => {
                    break doc.layers().iter().find(|x| x.id == l).map(|x| x.name.clone());
                }
                Some(amalith_core::ObjectParent::Group(g)) => walk = g,
                // Isolated straight into a symbol definition's own content
                // (Edit Symbol) rather than a layer — show its name instead.
                Some(amalith_core::ObjectParent::Symbol(s)) => {
                    break doc.symbol(s).map(|d| d.name.clone());
                }
                None => break None,
            }
        };
        out.push(layer_name.unwrap_or_else(|| "Layer".into()));
        for &id in &self.isolation {
            let name = doc
                .object(id)
                .and_then(|o| o.name.clone())
                .unwrap_or_else(|| match doc.object(id).map(|o| &o.kind) {
                    Some(amalith_core::ObjectKind::Group(g)) if g.clip.is_some() => {
                        "Clip Group".into()
                    }
                    Some(amalith_core::ObjectKind::Group(_)) => "Group".into(),
                    Some(amalith_core::ObjectKind::Path(_)) => "Path".into(),
                    Some(amalith_core::ObjectKind::CompoundPath(_)) => "Compound Path".into(),
                    Some(amalith_core::ObjectKind::Image(_)) => "Image".into(),
                    Some(amalith_core::ObjectKind::Symbol(data)) => doc
                        .symbol(data.definition)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| "Symbol".into()),
                    Some(amalith_core::ObjectKind::Text(_)) => "Type".into(),
                    Some(amalith_core::ObjectKind::Unknown { .. }) | None => "Object".into(),
                });
            out.push(name);
        }
        out
    }

    /// Doc-space points of the line to show while hovering one of a
    /// blend's generated steps (not its two original shapes) — the
    /// straight line between their centers, or the real spine polyline
    /// once one is set. Only while isolated directly into that blend
    /// group, matching how its steps are only ever reachable there in
    /// the first place (they're deliberately excluded from normal
    /// hit-testing, so this checks their bounds directly instead of
    /// going through `select::topmost_in`).
    pub(in crate::app) fn blend_spine_hover(&self) -> Option<Vec<amalith_core::Point>> {
        if !matches!(self.drag, Drag::None) {
            return None;
        }
        let group_id = self.isolation_root()?;
        let doc = self.doc.editor.document();
        let amalith_core::ObjectKind::Group(g) = &doc.object(group_id)?.kind else {
            return None;
        };
        let blend = g.blend?;
        let dp = self.doc_point(self.pointer);
        let over_a_step = g.children.iter().any(|&id| {
            id != blend.start
                && id != blend.end
                && select::bounds(doc, id).is_some_and(|b| b.contains(dp))
        });
        if !over_a_step {
            return None;
        }
        match blend.spine {
            Some(spine_id) => {
                let obj = doc.object(spine_id)?;
                let amalith_core::ObjectKind::Path(p) = &obj.kind else {
                    return None;
                };
                let xf = doc.world_transform(spine_id);
                p.flattened_points(0.5)
                    .into_iter()
                    .next()
                    .map(|pts| pts.into_iter().map(|pt| xf * pt).collect())
            }
            None => {
                let a = doc.bounds_of(blend.start)?.center();
                let b = doc.bounds_of(blend.end)?.center();
                Some(vec![a, b])
            }
        }
    }

    /// Object ▸ Clipping Mask ▸ Make (⌘7) — wrap the selection in a clip
    /// group masked by its topmost member.
    pub(in crate::app) fn clip_make(&mut self) {
        if self.doc.selection.len() < 2 {
            return;
        }
        if let Ok(CommandOutcome::Object(g)) = self.doc.editor.execute(Command::ClipMake {
            objects: self.doc.selection.clone(),
            name: None,
        }) {
            self.doc.selection = vec![g];
        }
        self.request_main_redraw();
    }

    /// Object ▸ Clipping Mask ▸ Release (⌘⌥7) — dissolve every selected
    /// clip group.
    pub(in crate::app) fn clip_release(&mut self) {
        let groups: Vec<ObjectId> = self
            .doc
            .selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Group(g)) if g.clip.is_some()
                )
            })
            .collect();
        if groups.is_empty() {
            return;
        }
        let mut freed = Vec::new();
        for g in groups {
            if let Ok(CommandOutcome::Object(id)) =
                self.doc.editor.execute(Command::ClipRelease { group: g })
            {
                freed.push(id);
            }
        }
        if !freed.is_empty() {
            self.doc.selection = freed;
        }
        self.prune_selection();
        self.request_main_redraw();
    }

    /// Object ▸ Group (⌘G) — wrap 2+ selected objects in a new group.
    /// Same logic the ⌘G keyboard shortcut already runs
    /// (`app/input/keyboard.rs`); pulled out here so the menu item and the
    /// shortcut share one implementation.
    pub(in crate::app) fn group_selection(&mut self) {
        if self.doc.selection.len() <= 1 {
            return;
        }
        if let Ok(CommandOutcome::Object(g)) = self.doc.editor.execute(Command::Group {
            ids: self.doc.selection.clone(),
            name: None,
        }) {
            self.doc.selection = vec![g];
        }
        self.request_main_redraw();
    }

    /// Object ▸ Ungroup (⌘⇧G) — dissolve every selected group. Same logic
    /// the ⌘⇧G keyboard shortcut already runs (`app/input/keyboard.rs`).
    pub(in crate::app) fn ungroup_selection(&mut self) {
        if let Ok(freed) = self.doc.editor.ungroup(&self.doc.selection) {
            if !freed.is_empty() {
                self.doc.selection = freed;
            }
        }
        self.request_main_redraw();
    }

    /// Object ▸ Lock ▸ Selection (⌘2) — locks every selected object; a
    /// locked object can't stay selected (matches the Layers panel's own
    /// lock toggle), so they drop out of the selection too.
    pub(in crate::app) fn lock_selection(&mut self) {
        if self.doc.selection.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut self.doc.selection);
        let _ = self.doc.editor.execute(Command::SetLocked { objects: ids, locked: true });
        self.request_main_redraw();
    }

    /// Object ▸ Unlock All (⌘⌥2) — unlocks every locked object in the
    /// whole document (not just the selection, there isn't one to speak
    /// of once something's locked) and selects all of them.
    pub(in crate::app) fn unlock_all(&mut self) {
        let ids: Vec<ObjectId> = self
            .doc
            .editor
            .document()
            .objects()
            .filter(|o| o.locked)
            .map(|o| o.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let _ = self.doc.editor.execute(Command::SetLocked { objects: ids.clone(), locked: false });
        self.doc.selection = ids;
        self.request_main_redraw();
    }

    /// Symbols panel "New Symbol" / Object ▸ Symbol Instance — converts
    /// the current selection into a new pooled symbol definition, in
    /// place. No-op on an empty selection.
    pub(in crate::app) fn define_symbol_from_selection(&mut self) {
        if self.doc.selection.is_empty() {
            return;
        }
        if let Ok(CommandOutcome::Object(instance)) = self.doc.editor.execute(Command::DefineSymbol {
            ids: self.doc.selection.clone(),
            name: None,
        }) {
            self.doc.selection = vec![instance];
            if let Some(amalith_core::ObjectKind::Symbol(data)) =
                self.doc.editor.document().object(instance).map(|o| &o.kind)
            {
                self.doc.selected_symbol = Some(data.definition);
            }
        }
        self.request_main_redraw();
    }

    /// Symbols panel footer "Place" — a new instance of `symbol`, centered
    /// on the current view.
    pub(in crate::app) fn place_symbol_instance(&mut self, symbol: amalith_core::SymbolId) {
        let Some(layer) = self.doc.selected_layer.or_else(|| self.doc.editor.document().layers().last().map(|l| l.id)) else {
            return;
        };
        let at = crate::convert::point_to_core(self.visible_doc_rect().center());
        if let Ok(CommandOutcome::Object(instance)) =
            self.doc.editor.execute(Command::PlaceSymbolInstance { symbol, layer, at })
        {
            self.doc.selection = vec![instance];
            self.request_main_redraw();
        }
    }

    /// Symbols panel footer "Edit" (Edit Symbol) — isolate into an
    /// existing instance of `symbol` if one is placed anywhere in the
    /// document; otherwise place a fresh one (centered on the current
    /// view, same as the footer's own "Place") and isolate into that
    /// instead. Isolation is scoped to a real placed object — a
    /// definition with zero instances has nothing to isolate *into* —
    /// but the definition itself is exactly as editable either way, so
    /// "no instances yet" shouldn't be a dead end: this is the one path
    /// that lets you edit *any* symbol, used or not.
    pub(in crate::app) fn edit_symbol_definition(&mut self, symbol: amalith_core::SymbolId) {
        let doc = self.doc.editor.document();
        let is_this_symbol = |o: &amalith_core::Object| {
            matches!(&o.kind, amalith_core::ObjectKind::Symbol(data) if data.definition == symbol)
        };
        // Prefer whichever instance you actually selected (e.g. just
        // placed) over an arbitrary one — the document's own object order
        // has nothing to do with what's currently on screen or which
        // instance you meant, so falling back to "the first match found"
        // could just as easily land you back in a *different* instance
        // (very often the original) instead of the one you were looking
        // at when you clicked Edit.
        let instance = self
            .doc.selection
            .iter()
            .find_map(|&id| doc.object(id).filter(|o| is_this_symbol(o)).map(|_| id))
            .or_else(|| doc.objects().find_map(|o| is_this_symbol(o).then_some(o.id)));
        let instance = instance.or_else(|| {
            let layer = self
                .doc.selected_layer
                .or_else(|| self.doc.editor.document().layers().last().map(|l| l.id))?;
            let at = crate::convert::point_to_core(self.visible_doc_rect().center());
            match self.doc.editor.execute(Command::PlaceSymbolInstance { symbol, layer, at }) {
                Ok(CommandOutcome::Object(id)) => Some(id),
                _ => None,
            }
        });
        if let Some(id) = instance {
            self.enter_isolation(id);
        }
    }

    /// Symbols panel footer "Delete" — removes the definition, breaking
    /// every remaining instance first (see `Command::DeleteSymbolDefinition`).
    pub(in crate::app) fn delete_symbol_definition(&mut self, symbol: amalith_core::SymbolId) {
        let _ = self.doc.editor.execute(Command::DeleteSymbolDefinition { id: symbol });
        if self.doc.selected_symbol == Some(symbol) {
            self.doc.selected_symbol = None;
        }
        self.request_main_redraw();
    }

    /// Whether the selection can be made into a clip group / has one to
    /// release — for enabling the Object menu items.
    pub(in crate::app) fn clip_state(&self) -> (bool, bool) {
        // Make needs 2+ objects whose frontmost is a plain shape.
        let can_make = self.doc.selection.len() >= 2
            && self.frontmost_selected().is_some_and(|id| {
                matches!(
                    self.doc.editor.document().object(id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Path(_) | amalith_core::ObjectKind::CompoundPath(_))
                )
            });
        let can_release = self.doc.selection.iter().any(|id| {
            matches!(
                self.doc.editor.document().object(*id).map(|o| &o.kind),
                Some(amalith_core::ObjectKind::Group(g)) if g.clip.is_some()
            )
        });
        (can_make, can_release)
    }
}
