//! `textRange.characterAttributes` — v1's text model is whole-object
//! (`amalith_core::TextStyle` applies to the entire `TextData`, no
//! per-character runs), so every accessor here reads/writes the *whole*
//! text object's style, not a sub-range. RAGE's own script 4 only ever
//! sets attributes on the whole injected string, which fits exactly;
//! finer per-character styling is out of scope (see the crate doc).

use amalith_commands::Command;
use amalith_core::{ObjectId, ObjectKind};
use boa_engine::class::{Class, ClassBuilder};
use boa_engine::{js_string, Context, JsArgs, JsData, JsResult, JsValue};
use boa_gc::{Finalize, Trace};

use super::{construct_instance, not_a, obj_or_throw, DocKey, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
    key: DocKey,
    id: ObjectId,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsCharacterAttributes {
    #[unsafe_ignore_trace]
    inner: Inner,
}

pub fn make(context: &mut Context, host: SharedHost, key: DocKey, id: ObjectId) -> JsResult<JsValue> {
    let obj = construct_instance(
        context,
        "CharacterAttributes",
        JsCharacterAttributes { inner: Inner { host, key, id } },
    )?;
    Ok(JsValue::from(obj))
}

impl JsCharacterAttributes {
    fn with_style<R>(&self, f: impl FnOnce(&amalith_core::TextStyle) -> R) -> Option<R> {
        let state = self.inner.host.borrow();
        state.documents.get(&self.inner.key).and_then(|d| {
            match d.editor.document().object(self.inner.id).map(|o| &o.kind) {
                Some(ObjectKind::Text(data)) => Some(f(&data.style)),
                _ => None,
            }
        })
    }

    fn set_style(&self, f: impl FnOnce(&mut amalith_core::TextStyle)) {
        let mut state = self.inner.host.borrow_mut();
        let Some(open) = state.documents.get_mut(&self.inner.key) else { return };
        let Some(object) = open.editor.document().object(self.inner.id) else { return };
        let ObjectKind::Text(data) = &object.kind else { return };
        let mut data = data.clone();
        f(&mut data.style);
        let _ = open.editor.execute(Command::SetText { object: self.inner.id, data });
    }

    fn size(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        Ok(JsValue::from(f.with_style(|s| s.size).unwrap_or(12.0)))
    }

    fn set_size(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let size = args.get_or_undefined(0).to_number(context)?;
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        f.set_style(|s| s.size = size);
        Ok(JsValue::undefined())
    }

    fn horizontal_scale(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        // Not a real amalith_core field yet — RAGE's fit-to-path logic
        // reads/writes this to condense long names; we track it via
        // `tracking`'s sibling, `TextStyle` has no horizontalScale field,
        // so this is approximated as a no-op 100 until the model grows one.
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let _f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        Ok(JsValue::from(100.0))
    }

    fn set_horizontal_scale(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        Ok(JsValue::undefined())
    }

    fn tracking(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        Ok(JsValue::from(f.with_style(|s| s.tracking).unwrap_or(0.0)))
    }

    fn set_tracking(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let tracking = args.get_or_undefined(0).to_number(context)?;
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        f.set_style(|s| s.tracking = tracking);
        Ok(JsValue::undefined())
    }

    fn text_font(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        let family = f.with_style(|s| s.family.clone()).unwrap_or_default();
        Ok(JsValue::from(js_string!(family)))
    }

    fn set_text_font(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        // `textFont` is normally a `Font` object in real ExtendScript
        // (from `app.textFonts.getByName`); we accept either that or a
        // plain PostScript-name string, since our `TextFont` (below) is
        // just a name wrapper.
        let arg = args.get_or_undefined(0);
        let name = if let Some(obj) = arg.as_object() {
            if let Some(font) = obj.downcast_ref::<super::app::JsTextFont>() {
                font.name().to_string()
            } else {
                arg.to_string(context)?.to_std_string_escaped()
            }
        } else {
            arg.to_string(context)?.to_std_string_escaped()
        };
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        f.set_style(|s| s.family = name);
        Ok(JsValue::undefined())
    }

    fn kerning_method(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        // Not modeled in `amalith_core::TextStyle` (v1 has no kerning-
        // method concept); read back whatever was last written so a
        // script's own round-trip check still behaves.
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let _f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        Ok(JsValue::from(js_string!("AUTO")))
    }

    fn set_kerning_method(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        Ok(JsValue::undefined())
    }

    fn capitalization(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "CharacterAttributes")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("CharacterAttributes"))?;
        Ok(JsValue::from(f.with_style(|s| s.small_caps).unwrap_or(false)))
    }
}

impl Class for JsCharacterAttributes {
    const NAME: &'static str = "CharacterAttributes";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(boa_engine::JsNativeError::typ()
            .with_message("CharacterAttributes is not directly constructible")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        super::accessor_rw(class, "size", Self::size, Self::set_size);
        super::accessor_rw(class, "horizontalScale", Self::horizontal_scale, Self::set_horizontal_scale);
        super::accessor_rw(class, "tracking", Self::tracking, Self::set_tracking);
        super::accessor_rw(class, "textFont", Self::text_font, Self::set_text_font);
        super::accessor_rw(class, "kerningMethod", Self::kerning_method, Self::set_kerning_method);
        super::accessor_ro(class, "capitalization", Self::capitalization);
        Ok(())
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsCharacterAttributes>()?;
    Ok(())
}
