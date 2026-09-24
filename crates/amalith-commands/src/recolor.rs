//! Selection-scoped color reassignment. Shared gradients are copied before editing.
use crate::{edit::Edit, CommandError};
use amalith_core::{AppearanceItem, Color, Document, GradientId, ObjectId, ObjectKind, Paint};
use std::collections::{HashMap, HashSet};

fn targets(doc: &Document, roots: &[ObjectId]) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(object) = doc.object(id) else {
            continue;
        };
        // Symbol definitions are shared; never rewrite them through an instance.
        // Images and adjustments have no fills or strokes to recolor.
        if matches!(object.kind, ObjectKind::Symbol(_) | ObjectKind::Image(_) | ObjectKind::Adjustment(_)) {
            continue;
        }
        ids.push(id);
        if let ObjectKind::Group(group) = &object.kind {
            pending.extend(&group.children);
        }
    }
    ids
}

/// Stable source palette, preserving exact float colors and alpha.
pub fn colors(doc: &Document, roots: &[ObjectId]) -> Vec<Color> {
    let mut out = Vec::new();
    let mut add = |c| {
        if !out.contains(&c) {
            out.push(c);
        }
    };
    for id in targets(doc, roots) {
        for item in &doc.object(id).unwrap().appearance.items {
            match item.paint() {
                Paint::Solid(c) => add(c),
                Paint::Gradient(id) => {
                    if let Some(g) = doc.gradient(id) {
                        for s in &g.stops {
                            add(s.color);
                        }
                        for p in &g.points {
                            add(p.color);
                        }
                    }
                }
                Paint::None => {}
            }
        }
    }
    out
}

pub(crate) fn compile(
    doc: &Document,
    roots: &[ObjectId],
    mapping: &[(Color, Color)],
) -> Result<Vec<Edit>, CommandError> {
    for &id in roots {
        if doc.object(id).is_none() {
            return Err(CommandError::ObjectNotFound(id));
        }
    }
    let replace = |c: Color| {
        mapping
            .iter()
            .find(|(from, _)| *from == c)
            .map_or(c, |(_, to)| Color { a: c.a, ..*to })
    };
    let mut edits = Vec::new();
    let mut gradients = HashMap::new();
    for id in targets(doc, roots) {
        let original = &doc.object(id).unwrap().appearance.items;
        let mut items = original.clone();
        for item in &mut items {
            let (AppearanceItem::Fill { paint, .. } | AppearanceItem::Stroke { paint, .. }) = item;
            match *paint {
                Paint::Solid(c) => *paint = Paint::Solid(replace(c)),
                Paint::Gradient(gid) => {
                    if let Some(&new_id) = gradients.get(&gid) {
                        *paint = Paint::Gradient(new_id);
                        continue;
                    }
                    let Some(old) = doc.gradient(gid) else {
                        continue;
                    };
                    let mut new = old.clone();
                    for s in &mut new.stops {
                        s.color = replace(s.color);
                    }
                    for p in &mut new.points {
                        p.color = replace(p.color);
                    }
                    if new != *old {
                        new.id = GradientId::new();
                        gradients.insert(gid, new.id);
                        *paint = Paint::Gradient(new.id);
                        edits.push(Edit::InsertGradient {
                            gradient: new,
                            index: doc.gradients().len() + gradients.len() - 1,
                        });
                    }
                }
                Paint::None => {}
            }
        }
        if items != *original {
            edits.push(Edit::SetAppearanceItems { id, items });
        }
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor};
    use amalith_core::{Gradient, Layer, LayerId, Object, ObjectParent};
    use kurbo::Rect;

    fn fixture(paint: Paint) -> (Editor, ObjectId, ObjectId) {
        let mut doc = Document::new("Recolor test");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Artwork"), 0);
        let ids = [ObjectId::new(), ObjectId::new()];
        for (index, id) in ids.into_iter().enumerate() {
            let mut object =
                Object::rectangle(id, ObjectParent::Layer(layer), Rect::new(0., 0., 10., 10.));
            object.appearance.set_fill(paint);
            doc.insert_object(object, index).unwrap();
        }
        (Editor::new(doc), ids[0], ids[1])
    }

    #[test]
    fn reassignment_preserves_alpha_selection_and_undo() {
        let red = Color::rgba(1., 0., 0., 0.4);
        let blue = Color::rgb(0., 0., 1.);
        let (mut editor, selected, other) = fixture(Paint::Solid(red));
        editor
            .execute(Command::RecolorArtwork {
                objects: vec![selected, selected],
                colors: vec![(red, blue), (blue, red)],
            })
            .unwrap();
        let expected = Paint::Solid(Color { a: red.a, ..blue });
        assert_eq!(
            editor
                .document()
                .object(selected)
                .unwrap()
                .appearance
                .fill(),
            expected
        );
        assert_eq!(
            editor.document().object(other).unwrap().appearance.fill(),
            Paint::Solid(red)
        );
        editor.undo().unwrap();
        assert_eq!(
            editor
                .document()
                .object(selected)
                .unwrap()
                .appearance
                .fill(),
            Paint::Solid(red)
        );
        editor.redo().unwrap();
        assert_eq!(
            editor
                .document()
                .object(selected)
                .unwrap()
                .appearance
                .fill(),
            expected
        );
    }

    #[test]
    fn shared_gradient_is_copied_and_undo_removes_copy() {
        for mut gradient in [
            Gradient::linear(GradientId::new()),
            Gradient::freeform(GradientId::new()),
        ] {
            let red = Color::rgba(1., 0., 0., 0.5);
            let blue = Color::rgb(0., 0., 1.);
            gradient.stops[0].color = red;
            if let Some(point) = gradient.points.first_mut() {
                point.color = red;
            }
            let gid = gradient.id;
            let (editor, selected, other) = fixture(Paint::Gradient(gid));
            let mut doc = editor.document().clone();
            doc.insert_gradient(gradient.clone(), 0);
            let mut editor = Editor::new(doc);
            editor
                .execute(Command::RecolorArtwork {
                    objects: vec![selected],
                    colors: vec![(red, blue)],
                })
                .unwrap();
            let Paint::Gradient(copy) = editor
                .document()
                .object(selected)
                .unwrap()
                .appearance
                .fill()
            else {
                panic!()
            };
            assert_ne!(copy, gid);
            assert_eq!(editor.document().gradient(gid), Some(&gradient));
            assert_eq!(
                editor.document().object(other).unwrap().appearance.fill(),
                Paint::Gradient(gid)
            );
            let changed = editor.document().gradient(copy).unwrap();
            assert_eq!(changed.stops[0].color, Color { a: red.a, ..blue });
            if let Some(point) = changed.points.first() {
                assert_eq!(point.color, Color { a: red.a, ..blue });
            }
            editor.undo().unwrap();
            assert!(editor.document().gradient(copy).is_none());
            assert_eq!(
                editor
                    .document()
                    .object(selected)
                    .unwrap()
                    .appearance
                    .fill(),
                Paint::Gradient(gid)
            );
            editor.redo().unwrap();
            assert!(editor.document().gradient(copy).is_some());
        }
    }
}
