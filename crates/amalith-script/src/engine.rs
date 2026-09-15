//! boa `Context` setup: strips ExtendScript-only preprocessor directives
//! that aren't valid JS, then wires up the host globals.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsResult, JsValue, NativeFunction, Source};

use crate::host::{fileio, HostState, SharedHost};

/// ExtendScript's `#target`/`#include`/`#strict`/`#script` lines are host
/// preprocessor directives, not JS — a real `#target illustrator` at the
/// top of every RAGE script would fail to parse as-is. Strip any line
/// whose first non-whitespace character is `#` before handing source to
/// boa. (No RAGE script uses `#` for anything else — bitwise/shebang `#`
/// usage doesn't occur in ExtendScript source.)
pub fn strip_directives(src: &str) -> String {
    src.lines()
        .map(|line| if line.trim_start().starts_with('#') { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a fresh `Context` with every host global registered, plus the
/// `HostState` backing it. One `Context`/`HostState` pair is meant to be
/// reused across a whole `run_pipeline` call so `$.global` (a plain JS
/// object, kept alive as long as the context's global environment
/// references it) and every open document stay shared across sequential
/// script loads, matching how `start.jsx` chains RAGE's six scripts.
pub fn new_context(cwd: PathBuf) -> JsResult<(Context, SharedHost)> {
    let mut context = Context::default();
    let host: SharedHost = Rc::new(RefCell::new(HostState::new(cwd)));

    fileio::register(&mut context)?;
    crate::host::textattrs::register(&mut context)?;
    crate::host::pageitem::register(&mut context)?;
    crate::host::artboard::register(&mut context)?;
    crate::host::layer::register(&mut context)?;
    crate::host::document::register(&mut context)?;
    crate::host::app::register(&mut context, host.clone())?;
    register_dollar(&mut context)?;
    register_text_type(&mut context)?;
    register_alert_stubs(&mut context)?;

    Ok((context, host))
}

/// `alert()`/`confirm()` — every real-world script uses at least `alert`;
/// this shim runs unattended, so `alert` just logs to stderr and `confirm`
/// always answers "yes" rather than blocking on stdin.
fn register_alert_stubs(context: &mut Context) -> JsResult<()> {
    fn alert(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        use boa_engine::JsArgs;
        let msg = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        eprintln!("[alert] {msg}");
        Ok(JsValue::undefined())
    }
    fn confirm(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        use boa_engine::JsArgs;
        let msg = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        eprintln!("[confirm, answering yes] {msg}");
        Ok(JsValue::from(true))
    }
    context.register_global_callable(js_string!("alert"), 1, NativeFunction::from_fn_ptr(alert))?;
    context.register_global_callable(js_string!("confirm"), 1, NativeFunction::from_fn_ptr(confirm))?;
    Ok(())
}

fn register_dollar(context: &mut Context) -> JsResult<()> {
    fn writeln(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        use boa_engine::JsArgs;
        let msg = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
        eprintln!("{msg}");
        Ok(JsValue::undefined())
    }

    let global_bag = ObjectInitializer::new(context).build();

    let dollar = ObjectInitializer::new(context)
        .property(js_string!("fileName"), js_string!(""), Attribute::all())
        .property(js_string!("global"), global_bag, Attribute::all())
        .function(NativeFunction::from_fn_ptr(writeln), js_string!("writeln"), 1)
        .build();

    context.register_global_property(js_string!("$"), dollar, Attribute::all())?;
    Ok(())
}

/// Set `$.fileName` before running one script, matching real ExtendScript's
/// per-file `$.fileName` — read by every RAGE script's `getScriptFolder()`.
pub fn set_current_file(context: &mut Context, path: &str) -> JsResult<()> {
    let dollar = context.global_object().get(js_string!("$"), context)?;
    if let Some(obj) = dollar.as_object() {
        obj.set(js_string!("fileName"), js_string!(path), true, context)?;
    }
    Ok(())
}

/// `TextType.{POINTTEXT,AREATEXT,PATHTEXT}` — matched by string against
/// `PageItem.kind` (see `host::pageitem::kind`), not by identity, so a
/// plain object of matching strings is enough.
fn register_text_type(context: &mut Context) -> JsResult<()> {
    let text_type = ObjectInitializer::new(context)
        .property(js_string!("POINTTEXT"), js_string!("POINTTEXT"), Attribute::all())
        .property(js_string!("AREATEXT"), js_string!("AREATEXT"), Attribute::all())
        .property(js_string!("PATHTEXT"), js_string!("PATHTEXT"), Attribute::all())
        .build();
    context.register_global_property(js_string!("TextType"), text_type, Attribute::all())?;

    let auto_kern = ObjectInitializer::new(context)
        .property(js_string!("AUTO"), js_string!("AUTO"), Attribute::all())
        .property(js_string!("NOAUTOKERN"), js_string!("NOAUTOKERN"), Attribute::all())
        .property(js_string!("OPTICAL"), js_string!("OPTICAL"), Attribute::all())
        .build();
    context.register_global_property(js_string!("AutoKernType"), auto_kern, Attribute::all())?;

    let save_options = ObjectInitializer::new(context)
        .property(js_string!("DONOTSAVECHANGES"), js_string!("DONOTSAVECHANGES"), Attribute::all())
        .property(js_string!("SAVECHANGES"), js_string!("SAVECHANGES"), Attribute::all())
        .build();
    context.register_global_property(js_string!("SaveOptions"), save_options, Attribute::all())?;
    Ok(())
}

pub fn eval_source(context: &mut Context, src: &str) -> JsResult<JsValue> {
    let stripped = strip_directives(src);
    context.eval(Source::from_bytes(stripped.as_bytes()))
}
