//! `Artboard`/`ArtboardList`. Artboards don't own objects in
//! `amalith-core` (a documented design choice — see `artboard.rs`'s
//! module doc — which happens to match real Illustrator, where "on an
//! artboard" is also a geometric query, not a stored edge). RAGE's own
//! scripts already do that intersection test themselves in JS against
//! `item.visibleBounds`/`artboard.artboardRect`, so no host-side helper
//! is needed here beyond exposing the rect.

use amalith_commands::Command;
use amalith_core::ArtboardId;
use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::{js_string, Context, JsData, JsResult, JsValue};
use boa_gc::{Finalize, Trace};

use super::{construct_instance, not_a, obj_or_throw, DocKey, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
    key: DocKey,
    id: ArtboardId,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsArtboard {
    #[unsafe_ignore_trace]
    inner: Inner,
}

pub fn make(context: &mut Context, host: SharedHost, key: DocKey, id: ArtboardId) -> JsResult<JsValue> {
    let obj = construct_instance(context, "Artboard", JsArtboard { inner: Inner { host, key, id } })?;
    Ok(JsValue::from(obj))
}

pub fn make_array(
    context: &mut Context,
    host: SharedHost,
    key: DocKey,
    ids: &[ArtboardId],
) -> JsResult<JsArray> {
    let mut values = Vec::with_capacity(ids.len());
    for &id in ids {
        values.push(make(context, host.clone(), key, id)?);
    }
    Ok(JsArray::from_iter(values, context))
}

impl JsArtboard {
    fn name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Artboard")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Artboard"))?;
        let state = f.inner.host.borrow();
        let name = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().artboard(f.inner.id))
            .map(|a| a.name.clone())
            .unwrap_or_default();
        Ok(JsValue::from(js_string!(name)))
    }

    fn set_name(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        use boa_engine::JsArgs;
        let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        let obj = obj_or_throw(this, "Artboard")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Artboard"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc.editor.execute(Command::RenameArtboard { id: f.inner.id, name });
        }
        Ok(JsValue::undefined())
    }

    fn artboard_rect(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Artboard")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Artboard"))?;
        let state = f.inner.host.borrow();
        let rect = state
            .documents
            .get(&f.inner.key)
            .and_then(|d| d.editor.document().artboard(f.inner.id))
            .map(|a| a.rect)
            .ok_or_else(|| not_a("Artboard (removed)"))?;
        let values = vec![
            JsValue::from(rect.x0),
            JsValue::from(rect.y0),
            JsValue::from(rect.x1),
            JsValue::from(rect.y1),
        ];
        Ok(JsValue::from(JsArray::from_iter(values, context)))
    }

    fn remove(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Artboard")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Artboard"))?;
        let mut state = f.inner.host.borrow_mut();
        if let Some(doc) = state.documents.get_mut(&f.inner.key) {
            let _ = doc.editor.execute(Command::DeleteArtboard { id: f.inner.id });
        }
        Ok(JsValue::undefined())
    }
}

impl Class for JsArtboard {
    const NAME: &'static str = "Artboard";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(boa_engine::JsNativeError::typ()
            .with_message("Artboard is not directly constructible")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(
            js_string!("remove"),
            0,
            boa_engine::NativeFunction::from_fn_ptr(Self::remove),
        );
        super::accessor_rw(class, "name", Self::name, Self::set_name);
        super::accessor_ro(class, "artboardRect", Self::artboard_rect);
        Ok(())
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsArtboard>()?;
    Ok(())
}
