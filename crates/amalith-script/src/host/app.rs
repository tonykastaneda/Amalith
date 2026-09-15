//! `app` — the one global singleton every script starts from.

use std::path::PathBuf;

use amalith_commands::Editor;
use amalith_core::Document;
use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsArgs, JsData, JsResult, JsValue, NativeFunction};
use boa_gc::{Finalize, Trace};

use super::{construct_instance, document, not_a, obj_or_throw, SharedHost};

#[derive(Debug)]
struct Inner {
    host: SharedHost,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsApp {
    #[unsafe_ignore_trace]
    inner: Inner,
}

fn file_or_string_path(args: &[JsValue], context: &mut Context) -> JsResult<PathBuf> {
    let arg = args.get_or_undefined(0);
    if let Some(obj) = arg.as_object() {
        if let Some(f) = obj.downcast_ref::<super::fileio::JsFile>() {
            return Ok(f.path().to_path_buf());
        }
    }
    Ok(PathBuf::from(arg.to_string(context)?.to_std_string_escaped()))
}

impl JsApp {
    fn documents(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let host = {
            let obj = obj_or_throw(this, "App")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("App"))?;
            f.inner.host.clone()
        };
        let mut keys: Vec<_> = host.borrow().documents.keys().copied().collect();
        keys.sort_unstable();
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            values.push(document::make(context, host.clone(), key)?);
        }
        let array = JsArray::from_iter(values, context);

        let host_for_add = host.clone();
        // SAFETY: the closure captures only `Rc<RefCell<HostState>>`, which
        // holds plain Rust data (no `JsValue`/`Gc<T>`) — nothing here needs
        // GC tracing, matching the crate doc's "live query, no boa state in
        // HostState" rule.
        let add_fn = unsafe {
            NativeFunction::from_closure(move |_this, _args, context| {
                let editor = Editor::new(Document::new("Untitled"));
                let key = host_for_add
                    .borrow_mut()
                    .insert_document(editor, amalith_io::AssetStore::new(), None);
                document::make(context, host_for_add.clone(), key)
            })
        };
        array.set(
            js_string!("add"),
            boa_engine::object::FunctionObjectBuilder::new(context.realm(), add_fn)
                .length(0)
                .name(js_string!("add"))
                .build(),
            true,
            context,
        )?;

        Ok(JsValue::from(array))
    }

    fn active_document(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let (host, active) = {
            let obj = obj_or_throw(this, "App")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("App"))?;
            let active = f.inner.host.borrow().active;
            (f.inner.host.clone(), active)
        };
        let key = active.ok_or_else(|| not_a("App (no active document)"))?;
        document::make(context, host, key)
    }

    fn open(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let path = file_or_string_path(args, context)?;
        let host = {
            let obj = obj_or_throw(this, "App")?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("App"))?;
            f.inner.host.clone()
        };
        let (doc, assets) = amalith_io::load(&path)
            .map_err(|e| boa_engine::JsNativeError::typ().with_message(format!("could not open {}: {e}", path.display())))?;
        let key = host.borrow_mut().insert_document(Editor::new(doc), assets, Some(path));
        document::make(context, host, key)
    }

    fn user_interaction_level(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = obj_or_throw(this, "App")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("App"))?;
        let non_interactive = f.inner.host.borrow().non_interactive;
        let s = if non_interactive { "DONTDISPLAYALERTS" } else { "" };
        Ok(JsValue::from(js_string!(s)))
    }

    fn set_user_interaction_level(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let s = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        let obj = obj_or_throw(this, "App")?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("App"))?;
        f.inner.host.borrow_mut().non_interactive = s == "DONTDISPLAYALERTS";
        Ok(JsValue::undefined())
    }
}

/// `app.textFonts.getByName(name)` — RAGE only ever passes the returned
/// `Font` straight back into `characterAttributes.textFont`, so this is
/// just a PostScript-name wrapper, not a real font-metrics lookup.
#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsTextFont {
    #[unsafe_ignore_trace]
    name: String,
}

impl JsTextFont {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

impl Class for JsTextFont {
    const NAME: &'static str = "Font";
    const LENGTH: usize = 1;

    fn data_constructor(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        Ok(JsTextFont { name })
    }

    fn init(_class: &mut ClassBuilder) -> JsResult<()> {
        Ok(())
    }
}

fn text_fonts_get_by_name(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let obj = construct_instance(context, "Font", JsTextFont { name })?;
    Ok(JsValue::from(obj))
}

impl Class for JsApp {
    const NAME: &'static str = "App";
    const LENGTH: usize = 0;

    fn data_constructor(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(boa_engine::JsNativeError::typ()
            .with_message("App is a singleton — use the global `app`")
            .into())
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(js_string!("open"), 1, NativeFunction::from_fn_ptr(Self::open));
        super::accessor_ro(class, "documents", Self::documents);
        super::accessor_ro(class, "activeDocument", Self::active_document);
        super::accessor_rw(
            class,
            "userInteractionLevel",
            Self::user_interaction_level,
            Self::set_user_interaction_level,
        );
        Ok(())
    }
}

pub fn register(context: &mut Context, host: SharedHost) -> JsResult<()> {
    context.register_global_class::<JsApp>()?;
    context.register_global_class::<JsTextFont>()?;
    let instance = construct_instance(context, "App", JsApp { inner: Inner { host } })?;

    let text_fonts = boa_engine::object::ObjectInitializer::new(context)
        .function(NativeFunction::from_fn_ptr(text_fonts_get_by_name), js_string!("getByName"), 1)
        .build();
    instance.set(js_string!("textFonts"), text_fonts, true, context)?;

    context.register_global_property(js_string!("app"), JsValue::from(instance), Attribute::all())?;

    let uil = boa_engine::object::ObjectInitializer::new(context)
        .property(
            js_string!("DONTDISPLAYALERTS"),
            js_string!("DONTDISPLAYALERTS"),
            Attribute::all(),
        )
        .build();
    context.register_global_property(js_string!("UserInteractionLevel"), uil, Attribute::all())?;
    Ok(())
}
