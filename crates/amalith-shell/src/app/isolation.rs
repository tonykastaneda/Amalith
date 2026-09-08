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

    /// Drop breadcrumb entries whose object no longer exists. Every level
    /// except the deepest must still be a group to drill through; the
    /// deepest may be a bare object (path / shape / image).
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
                Some(_) => *i + 1 == n,
            })
            .count();
        if good != self.isolation.len() {
            self.isolation.truncate(good);
        }
    }

    /// Enter (or drill deeper into) isolation on `id`. Any object except
    /// text can be isolated: a group opens its contents; a bare path,
    /// shape or image just dims everything else and scopes selection to
    /// itself.
    pub(in crate::app) fn enter_isolation(&mut self, id: ObjectId) {
        let is_group = match self.doc.editor.document().object(id).map(|o| &o.kind) {
            Some(amalith_core::ObjectKind::Text(_)) | None => return,
            Some(amalith_core::ObjectKind::Group(_)) => true,
            Some(_) => false,
        };
        if self.isolation.last() == Some(&id) {
            return;
        }
        self.isolation.push(id);
        self.doc.selection = if is_group { Vec::new() } else { vec![id] };
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
                    Some(amalith_core::ObjectKind::Symbol(_)) => "Symbol".into(),
                    Some(amalith_core::ObjectKind::Text(_)) => "Type".into(),
                    None => "Object".into(),
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
