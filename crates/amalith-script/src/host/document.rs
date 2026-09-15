//! `Document` — one open `(Editor, AssetStore)` pair, backed by
//! [`super::OpenDocument`] in [`super::HostState`].

use std::path::PathBuf;

use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::{js_string, Context, JsArgs, JsData, JsResult, JsValue};
use boa_gc::{Finalize, Trace};

use super::{artboard, construct_instance, layer, not_a, obj_or_throw, DocKey, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
    key: DocKey,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsDocument {
    #[unsafe_ignore_trace]
    inner: Inner,
}

pub fn make(context: &mut Context, host: SharedHost, key: DocKey) -> JsResult<JsValue> {
    let obj = construct_instance(context, "Document", JsDocument { inner: Inner { host, key } })?;
    Ok(JsValue::from(obj))
}

fn save_to(host: &SharedHost, key: DocKey, path: &std::path::Path) -> JsResult<()> {
    let mut state = host.borrow_mut();
    let open = state
        .documents
        .get_mut(&key)
        .ok_or_else(|| not_a("Document (closed)"))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "ai" {
        return Err(boa_engine::JsNativeError::typ()
            .with_message("saving to .ai is not supported — use a .amalith or .svg path")
            .into());
    }
    if ext == "svg" {
        let ids: Vec<_> = open
            .editor
            .document()
            .layers()
            .iter()
            .flat_map(|l| l.children.iter().copied())
            .collect();
        let svg = amalith_io::export_svg(open.editor.document(), &ids)
            .ok_or_else(|| boa_engine::JsNativeError::typ().with_message("could not export SVG"))?;
        std::fs::write(path, svg)
            .map_err(|e| boa_engine::JsNativeError::typ().with_message(format!("write failed: {e}")))?;
    } else {
        amalith_io::save(open.editor.document(), &open.assets, path)
            .map_err(|e| boa_engine::JsNativeError::typ().with_message(format!("save failed: {e}")))?;
    }
    open.full_name = Some(path.to_path_buf());
    open.saved = true;
    Ok(())
}

impl JsDocument {
    fn full_name(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        let full_name = {
            let state = f.inner.host.borrow();
            state.documents.get(&f.inner.key).and_then(|d| d.full_name.clone())
        };
        match full_name {
            Some(p) => {
                let file = super::fileio::JsFile::for_path(p);
                let obj = construct_instance(context, "File", file)?;
                Ok(JsValue::from(obj))
            }
            None => Ok(JsValue::null()),
        }
    }

    fn saved(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        let state = f.inner.host.borrow();
        let saved = state.documents.get(&f.inner.key).map(|d| d.saved).unwrap_or(true);
        Ok(JsValue::from(saved))
    }

    fn save(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        let path = {
            let state = f.inner.host.borrow();
            state
                .documents
                .get(&f.inner.key)
                .and_then(|d| d.full_name.clone())
                .ok_or_else(|| not_a("Document (never saved — use saveAs)"))?
        };
        save_to(&f.inner.host, f.inner.key, &path)?;
        Ok(JsValue::undefined())
    }

    fn save_as(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let path = file_arg_path(args, context)?;
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        save_to(&f.inner.host, f.inner.key, &path)?;
        Ok(JsValue::undefined())
    }

    fn close(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        f.inner.host.borrow_mut().close_document(f.inner.key);
        Ok(JsValue::undefined())
    }

    fn activate(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        f.inner.host.borrow_mut().active = Some(f.inner.key);
        Ok(JsValue::undefined())
    }

    /// `doc.selection = null` (RAGE's own usage — just clears it) or an
    /// array of `PageItem`s. There's no selection concept in
    /// `amalith-core::Document` itself (it's a GUI notion there); backed
    /// as plain state on `OpenDocument` so a preamble script (run before
    /// the real target script, in the same `run_pipeline` call) can stand
    /// in for "the user already selected something" headlessly.
    fn set_selection(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "Document")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
        let arg = args.get_or_undefined(0);
        let mut ids = Vec::new();
        if let Some(array_obj) = arg.as_object() {
            if let Ok(array) = JsArray::from_object(array_obj.clone()) {
                let len = array.length(context)?;
                for i in 0..len {
                    let item = array.get(i, context)?;
                    if let Some(item_obj) = item.as_object() {
                        if let Some(pi) = item_obj.downcast_ref::<super::pageitem::JsPageItem>() {
                            ids.push(pi.id());
                        }
                    }
                }
            }
        }
        let mut state = f.inner.host.borrow_mut();
        if let Some(open) = state.documents.get_mut(&f.inner.key) {
            open.selection = ids;
        }
        Ok(JsValue::undefined())
    }

    fn get_selection(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, ids) = {
            let obj = obj_or_throw(this, "Document")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
            let state = f.inner.host.borrow();
            let ids = state.documents.get(&f.inner.key).map(|d| d.selection.clone()).unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, ids)
        };
        let array = super::pageitem::make_array(context, host, key, &ids)?;
        Ok(JsValue::from(array))
    }

    fn layers(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, ids) = {
            let obj = obj_or_throw(this, "Document")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
            let state = f.inner.host.borrow();
            let ids = state
                .documents
                .get(&f.inner.key)
                .map(|d| d.editor.document().layers().iter().map(|l| l.id).collect::<Vec<_>>())
                .unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, ids)
        };
        let array = layer::make_array(context, host, key, &ids)?;
        Ok(JsValue::from(array))
    }

    fn artboards(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, key, ids) = {
            let obj = obj_or_throw(this, "Document")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Document"))?;
            let state = f.inner.host.borrow();
            let ids = state
                .documents
                .get(&f.inner.key)
                .map(|d| d.editor.document().artboards().iter().map(|a| a.id).collect::<Vec<_>>())
                .unwrap_or_default();
            (f.inner.host.clone(), f.inner.key, ids)
        };
        let array = artboard::make_array(context, host, key, &ids)?;
        Ok(JsValue::from(array))
    }
}

fn file_arg_path(args: &[JsValue], context: &mut Context) -> JsResult<PathBuf> {
    let arg = args.get_or_undefined(0);
    if let Some(obj) = arg.as_object() {
        if let Some(f) = obj.downcast_ref::<super::fileio::JsFile>() {
            return Ok(f.path().to_path_buf());
        }
    }
    let s = arg.to_string(context)?.to_std_string_escaped();
    Ok(PathBuf::from(s))
}

impl Class for JsDocument {
    const NAME: &'static str = "Document";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(boa_engine::JsNativeError::typ()
            .with_message("Document is not directly constructible — use app.documents.add()")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(js_string!("save"), 0, boa_engine::NativeFunction::from_fn_ptr(Self::save));
        class.method(js_string!("saveAs"), 1, boa_engine::NativeFunction::from_fn_ptr(Self::save_as));
        class.method(js_string!("close"), 0, boa_engine::NativeFunction::from_fn_ptr(Self::close));
        class.method(js_string!("activate"), 0, boa_engine::NativeFunction::from_fn_ptr(Self::activate));
        super::accessor_ro(class, "fullName", Self::full_name);
        super::accessor_ro(class, "saved", Self::saved);
        super::accessor_rw(class, "selection", Self::get_selection, Self::set_selection);
        super::accessor_ro(class, "layers", Self::layers);
        super::accessor_ro(class, "artboards", Self::artboards);
        Ok(())
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsDocument>()?;
    Ok(())
}
