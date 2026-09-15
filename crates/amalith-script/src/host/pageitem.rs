//! A single class standing in for every Illustrator page-item `typename`
//! (`PathItem`, `GroupItem`, `PlacedItem`, `RasterItem`, `TextFrame`, ...).
//!
//! Real Illustrator gives each `typename` its own prototype chain
//! (`instanceof` works); RAGE's scripts only ever branch on the *string*
//! `.typename`, never `instanceof`, so one Rust class computing `.typename`
//! live from the object's current `ObjectKind`/`Asset` state is behaviorally
//! equivalent for this shim's scope and far simpler than a full hierarchy —
//! it's also what makes `.typename` flip from `"PlacedItem"` to
//! `"RasterItem"` after `.embed()` fall out for free (see the crate doc).

use amalith_commands::{Command, CommandOutcome};
use amalith_core::{AppearanceItem, AssetSource, ObjectId, ObjectKind, ObjectParent, PathData, TextKind};
use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::object::ObjectInitializer;
use boa_engine::{js_string, Context, JsArgs, JsData, JsResult, JsValue, NativeFunction};
use boa_gc::{Finalize, Trace};

use super::{construct_instance, not_a, obj_or_throw, textattrs, DocKey, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
    key: DocKey,
    id: ObjectId,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsPageItem {
    #[unsafe_ignore_trace]
    inner: Inner,
}

pub fn make(context: &mut Context, host: SharedHost, key: DocKey, id: ObjectId) -> JsResult<JsValue> {
    let obj = construct_instance(context, "PageItem", JsPageItem { inner: Inner { host, key, id } })?;
    Ok(JsValue::from(obj))
}

impl JsPageItem {
    pub(crate) fn id(&self) -> ObjectId {
        self.inner.id
    }
}

/// Live array of `PageItem`s for a `Group`'s children (or a layer's
/// top-level children, via [`crate::host::layer`]).
pub fn make_array(
    context: &mut Context,
    host: SharedHost,
    key: DocKey,
    ids: &[ObjectId],
) -> JsResult<JsArray> {
    let mut values = Vec::with_capacity(ids.len());
    for &id in ids {
        values.push(make(context, host.clone(), key, id)?);
    }
    Ok(JsArray::from_iter(values, context))
}

fn kind_name(kind: &ObjectKind, host: &SharedHost, key: DocKey) -> String {
    match kind {
        ObjectKind::Path(_) => "PathItem".to_string(),
        ObjectKind::CompoundPath(_) => "CompoundPathItem".to_string(),
        ObjectKind::Group(_) => "GroupItem".to_string(),
        ObjectKind::Text(_) => "TextFrame".to_string(),
        ObjectKind::Symbol(_) => "SymbolItem".to_string(),
        ObjectKind::Image(data) => {
            let state = host.borrow();
            let embedded = state
                .documents
                .get(&key)
                .and_then(|d| d.editor.document().assets().iter().find(|a| a.id == data.asset))
                .map(|a| a.is_embedded())
                .unwrap_or(false);
            if embedded { "RasterItem".to_string() } else { "PlacedItem".to_string() }
        }
        ObjectKind::Unknown { .. } => "Unknown".to_string(),
    }
}

impl JsPageItem {
    fn typename(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let doc = &state
            .documents
            .get(&f.inner.key)
            .ok_or_else(|| not_a("PageItem (document closed)"))?
            .editor;
        let object = doc
            .document()
            .object(f.inner.id)
            .ok_or_else(|| not_a("PageItem (object removed)"))?;
        Ok(JsValue::from(js_string!(kind_name(&object.kind, &f.inner.host, f.inner.key))))
    }

    fn name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let name = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().object(f.inner.id))
            .and_then(|o| o.name.clone())
            .unwrap_or_default();
        Ok(JsValue::from(js_string!(name)))
    }

    fn set_name(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc
                .editor
                .execute(Command::RenameObject { id: f.inner.id, name: Some(name) });
        }
        Ok(JsValue::undefined())
    }

    fn locked(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let locked = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().object(f.inner.id))
            .map(|o| o.locked)
            .unwrap_or(false);
        Ok(JsValue::from(locked))
    }

    fn set_locked(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let locked = args.get_or_undefined(0).to_boolean();
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc.editor.execute(Command::SetLocked { objects: vec![f.inner.id], locked });
        }
        Ok(JsValue::undefined())
    }

    fn hidden(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let visible = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().object(f.inner.id))
            .map(|o| o.visible)
            .unwrap_or(true);
        Ok(JsValue::from(!visible))
    }

    fn set_hidden(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let hidden = args.get_or_undefined(0).to_boolean();
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc
                .editor
                .execute(Command::SetVisible { objects: vec![f.inner.id], visible: !hidden });
        }
        Ok(JsValue::undefined())
    }

    fn parent(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, parent) = {
            let obj = obj_or_throw(this, "PageItem")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
            let state = f.inner.host.borrow();
            let parent = state
                .documents
                .get(&f.inner.key)
                .and_then(|d| d.editor.document().object(f.inner.id))
                .map(|o| o.parent);
            (f.inner.host.clone(), f.inner.key, parent)
        };
        match parent {
            Some(ObjectParent::Layer(lid)) => super::layer::make(context, host, key, lid),
            Some(ObjectParent::Group(gid)) => make(context, host, key, gid),
            _ => Ok(JsValue::null()),
        }
    }

    fn layer(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, mut cursor) = {
            let obj = obj_or_throw(this, "PageItem")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
            (f.inner.host.clone(), f.inner.key, f.inner.id)
        };
        loop {
            let parent = {
                let state = host.borrow();
                state
                    .documents
                    .get(&key)
                    .and_then(|d| d.editor.document().object(cursor))
                    .map(|o| o.parent)
            };
            match parent {
                Some(ObjectParent::Layer(lid)) => return super::layer::make(context, host, key, lid),
                Some(ObjectParent::Group(gid)) => cursor = gid,
                _ => return Ok(JsValue::null()),
            }
        }
    }

    fn bounds(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let rect = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().bounds_of(f.inner.id))
            .ok_or_else(|| not_a("PageItem (no bounds)"))?;
        let values = vec![
            JsValue::from(rect.x0),
            JsValue::from(rect.y0),
            JsValue::from(rect.x1),
            JsValue::from(rect.y1),
        ];
        Ok(JsValue::from(JsArray::from_iter(values, context)))
    }

    fn page_items(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, children) = {
            let obj = obj_or_throw(this, "PageItem")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
            let state = f.inner.host.borrow();
            let children = state
                .documents
                .get(&f.inner.key)
                .map(|d| d.editor.document().children_of(ObjectParent::Group(f.inner.id)).to_vec())
                .unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, children)
        };
        let array = make_array(context, host, key, &children)?;
        Ok(JsValue::from(array))
    }

    fn remove(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc.editor.execute(Command::DeleteObject { id: f.inner.id });
        }
        Ok(JsValue::undefined())
    }

    // -------------------------------------------------------------
    // PlacedItem / RasterItem
    // -------------------------------------------------------------

    fn embedded(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let state = f.inner.host.borrow();
        let embedded = state.documents.get(&f.inner.key).and_then(|d| {
            let doc = d.editor.document();
            match doc.object(f.inner.id).map(|o| &o.kind) {
                Some(ObjectKind::Image(data)) => doc.asset(data.asset).map(|a| a.is_embedded()),
                _ => None,
            }
        });
        Ok(JsValue::from(embedded.unwrap_or(false)))
    }

    /// Copies a `Linked` asset's bytes into the document's own
    /// `AssetStore` and flips it to `Embedded` — mirrors
    /// `amalith-shell`'s `embed_asset`. No-op for anything that isn't a
    /// currently-linked image (already-embedded items, or non-images).
    fn embed(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let mut state = f.inner.host.borrow_mut();
        let Some(open) = state.documents.get_mut(&f.inner.key) else {
            return Ok(JsValue::undefined());
        };
        let Some(ObjectKind::Image(data)) = open.editor.document().object(f.inner.id).map(|o| &o.kind) else {
            return Ok(JsValue::undefined());
        };
        let asset_id = data.asset;
        let Some(asset) = open.editor.document().asset(asset_id).cloned() else {
            return Ok(JsValue::undefined());
        };
        let AssetSource::Linked { path, .. } = &asset.source else {
            return Ok(JsValue::undefined());
        };
        let Ok(bytes) = std::fs::read(path) else {
            return Ok(JsValue::undefined());
        };
        let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("png");
        let container = format!("images/{}-{asset_id}.{ext}", asset.name);
        open.assets.insert(&container, bytes);
        let _ = open.editor.execute(Command::SetAssetSource {
            id: asset_id,
            source: AssetSource::Embedded { container_path: container },
        });
        Ok(JsValue::undefined())
    }

    // -------------------------------------------------------------
    // TextFrame
    // -------------------------------------------------------------

    fn text_data(&self) -> Option<amalith_core::TextData> {
        let state = self.inner.host.borrow();
        state.documents.get(&self.inner.key).and_then(|d| {
            match d.editor.document().object(self.inner.id).map(|o| o.kind.clone()) {
                Some(ObjectKind::Text(data)) => Some(data),
                _ => None,
            }
        })
    }

    fn contents(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let content = f.text_data().map(|d| d.content).unwrap_or_default();
        Ok(JsValue::from(js_string!(content)))
    }

    fn set_contents(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let text = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let Some(mut data) = f.text_data() else { return Ok(JsValue::undefined()) };
        data.content = text;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc.editor.execute(Command::SetText { object: f.inner.id, data });
        }
        Ok(JsValue::undefined())
    }

    fn kind(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let s = match f.text_data().map(|d| d.kind) {
            Some(TextKind::Path(_)) => "PATHTEXT",
            Some(TextKind::Area { .. }) => "AREATEXT",
            Some(TextKind::Point) => "POINTTEXT",
            None => "",
        };
        Ok(JsValue::from(js_string!(s)))
    }

    fn text_range(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, id) = {
            let obj = obj_or_throw(this, "PageItem")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
            (f.inner.host.clone(), f.inner.key, f.inner.id)
        };
        let attrs = textattrs::make(context, host.clone(), key, id)?;
        let characters = JsArray::from_iter(Vec::new(), context);
        let obj = ObjectInitializer::new(context)
            .property(js_string!("characterAttributes"), attrs, boa_engine::property::Attribute::all())
            .property(js_string!("characters"), characters, boa_engine::property::Attribute::all())
            .build();
        Ok(JsValue::from(obj))
    }

    /// Read-only projection of the text object's path geometry (primary:
    /// `TextData.path_geometry`; legacy fallback: the separate object
    /// `PathTextData::path` points at) — `.closed`, `.pathPoints[i].anchor`,
    /// `.length` (via `amalith_core::ArcLengthPath`, the same helper the
    /// GUI's own `outline_path_text` uses).
    fn text_path(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, data) = {
            let obj = obj_or_throw(this, "PageItem")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
            (f.inner.host.clone(), f.inner.key, f.text_data())
        };
        let Some(data) = data else { return Ok(JsValue::undefined()) };

        let path_data = if let Some(pd) = &data.path_geometry {
            Some(pd.clone())
        } else if let TextKind::Path(pt) = &data.kind {
            let state = host.borrow();
            state.documents.get(&key).and_then(|d| match d.editor.document().object(pt.path).map(|o| o.kind.clone()) {
                Some(ObjectKind::Path(pd)) => Some(pd),
                _ => None,
            })
        } else {
            None
        };
        let Some(path_data) = path_data else { return Ok(JsValue::undefined()) };
        let Some(subpath) = path_data.subpaths().first() else { return Ok(JsValue::undefined()) };

        let mut points = Vec::with_capacity(subpath.anchors.len());
        for anchor in &subpath.anchors {
            let arr = JsArray::from_iter(
                vec![JsValue::from(anchor.point.x), JsValue::from(anchor.point.y)],
                context,
            );
            let point_obj = ObjectInitializer::new(context)
                .property(js_string!("anchor"), arr, boa_engine::property::Attribute::all())
                .build();
            points.push(JsValue::from(point_obj));
        }
        let path_points = JsArray::from_iter(points, context);

        let mut length = 0.0;
        for seg in path_data.geometry.segments() {
            length += kurbo::ParamCurveArclen::arclen(&seg, 0.1);
        }

        let obj = ObjectInitializer::new(context)
            .property(js_string!("closed"), subpath.closed, boa_engine::property::Attribute::all())
            .property(js_string!("pathPoints"), path_points, boa_engine::property::Attribute::all())
            .property(js_string!("length"), length, boa_engine::property::Attribute::all())
            .build();
        Ok(JsValue::from(obj))
    }

    /// `TextFrame.createOutline()` — mirrors `amalith-shell`'s
    /// `create_outlines()`: `CreatePath` → `SetTransform` →
    /// `SetFill`/`SetStroke`(+width/style) → reparent to match the text's
    /// own parent → `DeleteObject` on the original text. A no-op for any
    /// item that isn't text, or whose glyphs shape to an empty outline.
    fn create_outline(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "PageItem")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("PageItem"))?;
        let host = f.inner.host.clone();
        let key = f.inner.key;
        let id = f.inner.id;

        let Some(data) = f.text_data() else { return Ok(JsValue::undefined()) };

        let computed = {
            let mut state = host.borrow_mut();
            let Some(open) = state.documents.get_mut(&key) else { return Ok(JsValue::undefined()) };
            let Some(object) = open.editor.document().object(id) else { return Ok(JsValue::undefined()) };
            let transform = object.transform;
            let parent = object.parent;
            let appearance = object.appearance.clone();

            let mut cursor_parent = parent;
            let layer_id = loop {
                match cursor_parent {
                    ObjectParent::Layer(lid) => break Some(lid),
                    ObjectParent::Group(gid) => match open.editor.document().object(gid).map(|o| o.parent) {
                        Some(p) => cursor_parent = p,
                        None => break None,
                    },
                    ObjectParent::Symbol(_) => break None,
                }
            };
            let Some(layer_id) = layer_id else { return Ok(JsValue::undefined()) };

            let bez = crate::outline::outline_text_data(state.shaper(), &data);
            let fill = appearance.items.iter().find_map(|item| match item {
                AppearanceItem::Fill { paint, .. } => Some(*paint),
                _ => None,
            });
            let stroke = appearance.items.iter().find_map(|item| match item {
                AppearanceItem::Stroke { paint, width, style, .. } => Some((*paint, *width, style.clone())),
                _ => None,
            });
            (bez, layer_id, parent, transform, fill, stroke)
        };
        let (bez, layer_id, parent, transform, fill, stroke) = computed;

        if bez.elements().is_empty() {
            return Ok(JsValue::undefined());
        }

        let path_data = PathData::from_bezpath(bez);
        let mut state = host.borrow_mut();
        let Some(open) = state.documents.get_mut(&key) else { return Ok(JsValue::undefined()) };

        let Ok(CommandOutcome::Object(new_id)) =
            open.editor.execute(Command::CreatePath { layer: layer_id, path: path_data, name: None })
        else {
            return Ok(JsValue::undefined());
        };

        let _ = open.editor.execute(Command::SetTransform { object: new_id, transform });
        if let Some(paint) = fill {
            let _ = open.editor.execute(Command::SetFill { objects: vec![new_id], paint });
        }
        if let Some((paint, width, style)) = stroke {
            let _ = open.editor.execute(Command::SetStroke { objects: vec![new_id], paint });
            let _ = open.editor.execute(Command::SetStrokeWidth { objects: vec![new_id], width });
            let _ = open.editor.execute(Command::SetStrokeStyle { objects: vec![new_id], style });
        }
        if let ObjectParent::Group(gid) = parent {
            let index = open.editor.document().children_of(ObjectParent::Group(gid)).len();
            let _ = open.editor.execute(Command::Reparent { ids: vec![new_id], parent: ObjectParent::Group(gid), index });
        }
        let _ = open.editor.execute(Command::DeleteObject { id });

        Ok(JsValue::undefined())
    }
}

impl Class for JsPageItem {
    const NAME: &'static str = "PageItem";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        // Not constructible from JS directly — RAGE scripts never call
        // `new PageItem(...)`, only ever receive them from collections.
        Err(boa_engine::JsNativeError::typ()
            .with_message("PageItem is not constructible")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(js_string!("remove"), 0, NativeFunction::from_fn_ptr(Self::remove));
        class.method(js_string!("embed"), 0, NativeFunction::from_fn_ptr(Self::embed));
        class.method(js_string!("createOutline"), 0, NativeFunction::from_fn_ptr(Self::create_outline));
        super::accessor_ro(class, "typename", Self::typename);
        super::accessor_rw(class, "name", Self::name, Self::set_name);
        super::accessor_rw(class, "locked", Self::locked, Self::set_locked);
        super::accessor_rw(class, "hidden", Self::hidden, Self::set_hidden);
        super::accessor_ro(class, "parent", Self::parent);
        super::accessor_ro(class, "layer", Self::layer);
        super::accessor_ro(class, "visibleBounds", Self::bounds);
        super::accessor_ro(class, "geometricBounds", Self::bounds);
        super::accessor_ro(class, "pageItems", Self::page_items);
        super::accessor_ro(class, "embedded", Self::embedded);
        super::accessor_rw(class, "contents", Self::contents, Self::set_contents);
        super::accessor_ro(class, "kind", Self::kind);
        super::accessor_ro(class, "textRange", Self::text_range);
        super::accessor_ro(class, "textPath", Self::text_path);
        Ok(())
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsPageItem>()?;
    Ok(())
}
