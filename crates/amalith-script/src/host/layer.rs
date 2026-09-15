//! `Layer` (top-level only — see [`crate::host::document`] and the crate
//! doc for how nested `Layer.layers` is approximated via `Group` objects,
//! since `amalith-core::Document` has no real sublayer nesting).

use amalith_commands::{Command, LayerOptions};
use amalith_core::{LayerId, ObjectParent};
use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::{js_string, Context, JsData, JsResult, JsValue};
use boa_gc::{Finalize, Trace};

use super::{construct_instance, not_a, obj_or_throw, pageitem, DocKey, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
    key: DocKey,
    id: LayerId,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsLayer {
    #[unsafe_ignore_trace]
    inner: Inner,
}

pub fn make(context: &mut Context, host: SharedHost, key: DocKey, id: LayerId) -> JsResult<JsValue> {
    let obj = construct_instance(context, "Layer", JsLayer { inner: Inner { host, key, id } })?;
    Ok(JsValue::from(obj))
}

pub fn make_array(context: &mut Context, host: SharedHost, key: DocKey, ids: &[LayerId]) -> JsResult<JsArray> {
    let mut values = Vec::with_capacity(ids.len());
    for &id in ids {
        values.push(make(context, host.clone(), key, id)?);
    }
    Ok(JsArray::from_iter(values, context))
}

impl JsLayer {
    fn with_options<R>(&self, f: impl FnOnce(&amalith_core::Layer) -> R) -> Option<R> {
        let state = self.inner.host.borrow();
        state
            .documents
            .get(&self.inner.key)
            .and_then(|d| d.editor.document().layer(self.inner.id))
            .map(f)
    }

    fn set_options(&self, f: impl FnOnce(&mut LayerOptions)) {
        let current = self.with_options(|l| LayerOptions {
            name: l.name.clone(),
            color: l.color,
            visible: l.visible,
            locked: l.locked,
            template: l.template,
            print: l.print,
            preview: l.preview,
            dim_images_to: l.dim_images_to,
        });
        let Some(mut options) = current else { return };
        f(&mut options);
        let mut state = self.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&self.inner.key) {
            let _ = doc.editor.execute(Command::SetLayerOptions { id: self.inner.id, options });
        }
    }

    fn typename(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        Ok(JsValue::from(js_string!("Layer")))
    }

    fn name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
        let name = f.with_options(|l| l.name.clone()).unwrap_or_default();
        Ok(JsValue::from(js_string!(name)))
    }

    fn visible(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
        Ok(JsValue::from(f.with_options(|l| l.visible).unwrap_or(true)))
    }

    fn set_visible(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let visible = args.first().map(|v| v.to_boolean()).unwrap_or(true);
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
        f.set_options(|o| o.visible = visible);
        Ok(JsValue::undefined())
    }

    fn locked(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
        Ok(JsValue::from(f.with_options(|l| l.locked).unwrap_or(false)))
    }

    fn set_locked(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let locked = args.first().map(|v| v.to_boolean()).unwrap_or(false);
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
        f.set_options(|o| o.locked = locked);
        Ok(JsValue::undefined())
    }

    /// `Layer.layers` — RAGE only ever *reads* sublayers recursively; since
    /// `amalith-core` has no real sublayer nesting, every top-level `Group`
    /// child of this layer is exposed here as a pseudo-sublayer AND (via
    /// `pageItems`) as a `GroupItem` — a documented approximation, see the
    /// crate doc. Sublayer *creation* is out of scope.
    fn layers(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, group_ids) = {
            let obj = obj_or_throw(this, "Layer")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
            let state = f.inner.host.borrow();
            let ids = state
                .documents
                .get(&f.inner.key)
                .map(|d| {
                    d.editor
                        .document()
                        .children_of(ObjectParent::Layer(f.inner.id))
                        .iter()
                        .filter(|&&id| {
                            matches!(
                                d.editor.document().object(id).map(|o| &o.kind),
                                Some(amalith_core::ObjectKind::Group(_))
                            )
                        })
                        .copied()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, ids)
        };
        let array = pageitem::make_array(context, host, key, &group_ids)?;
        Ok(JsValue::from(array))
    }

    fn page_items(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, ids) = {
            let obj = obj_or_throw(this, "Layer")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Layer"))?;
            let state = f.inner.host.borrow();
            let ids = state
                .documents
                .get(&f.inner.key)
                .map(|d| d.editor.document().children_of(ObjectParent::Layer(f.inner.id)).to_vec())
                .unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, ids)
        };
        let array = pageitem::make_array(context, host, key, &ids)?;
        Ok(JsValue::from(array))
    }

    /// `Layer.placedItems`/`rasterItems`: real Illustrator splits linked vs.
    /// embedded images into two distinct typed collections. Both return
    /// every `Image` object here rather than filtering by embed state —
    /// harmless for RAGE's scripts, which always re-check `.embedded` (or
    /// `.typename`) on each item themselves before acting, and `.embed()`
    /// on an already-embedded item is a no-op (Phase 3).
    fn placed_items(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        filter_kind(this, args, context, |k| {
            matches!(k, amalith_core::ObjectKind::Image(_))
        })
    }

    fn raster_items(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        filter_kind(this, args, context, |k| {
            matches!(k, amalith_core::ObjectKind::Image(_))
        })
    }
}

fn filter_kind(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
    pred: impl Fn(&amalith_core::ObjectKind) -> bool,
) -> JsResult<JsValue> {
    let (host, key, ids) = {
        let obj = obj_or_throw(this, "Layer")?;
        let f = obj.downcast_ref::<JsLayer>().ok_or_else(|| not_a("Layer"))?;
        let state = f.inner.host.borrow();
        let ids = state
            .documents
            .get(&f.inner.key)
            .map(|d| {
                d.editor
                    .document()
                    .children_of(ObjectParent::Layer(f.inner.id))
                    .iter()
                    .filter(|&&id| d.editor.document().object(id).is_some_and(|o| pred(&o.kind)))
                    .copied()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        (f.inner.host.clone(), f.inner.key, ids)
    };
    let array = pageitem::make_array(context, host, key, &ids)?;
    Ok(JsValue::from(array))
}

impl Class for JsLayer {
    const NAME: &'static str = "Layer";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(boa_engine::JsNativeError::typ()
            .with_message("Layer is not constructible")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        super::accessor_ro(class, "typename", Self::typename);
        super::accessor_ro(class, "name", Self::name);
        super::accessor_rw(class, "visible", Self::visible, Self::set_visible);
        super::accessor_rw(class, "locked", Self::locked, Self::set_locked);
        super::accessor_ro(class, "layers", Self::layers);
        super::accessor_ro(class, "pageItems", Self::page_items);
        super::accessor_ro(class, "placedItems", Self::placed_items);
        super::accessor_ro(class, "rasterItems", Self::raster_items);
        Ok(())
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsLayer>()?;
    Ok(())
}
