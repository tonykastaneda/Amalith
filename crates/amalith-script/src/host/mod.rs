//! Host state shared by every native object exposed to scripts.
//!
//! Deliberately holds nothing boa-managed (no `JsValue`/`JsObject`): every
//! field here is plain Rust data (an `Editor`, an `AssetStore`, paths,
//! flags). That's what lets every host wrapper struct's `Trace` impl be a
//! genuine no-op (`#[unsafe_ignore_trace]` on the single field pointing
//! back here) — this state lives outside boa's GC graph entirely, reached
//! only *from* native function bodies, never traversed *by* the collector.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use amalith_commands::Editor;
use amalith_io::AssetStore;
use boa_engine::object::NativeObject;
use boa_engine::{js_string, Context, JsNativeError, JsObject, JsResult, JsValue};

pub mod app;
pub mod artboard;
pub mod document;
pub mod fileio;
pub mod layer;
pub mod pageitem;
pub mod textattrs;

pub fn not_a(name: &str) -> boa_engine::JsError {
    JsNativeError::typ()
        .with_message(format!("not a {name}"))
        .into()
}

/// Build a new instance of an already-registered native class from Rust
/// code (mirrors what `new ClassName(...)` does in JS, without re-running
/// the JS constructor): look up the class's constructor on the global
/// object, take its `.prototype`, and attach `data` as the instance's
/// native payload.
pub fn construct_instance<T: NativeObject>(
    context: &mut Context,
    class_name: &str,
    data: T,
) -> JsResult<JsObject> {
    let ctor = context
        .global_object()
        .get(js_string!(class_name), context)?;
    let ctor_obj = ctor.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message(format!("`{class_name}` is not registered"))
    })?;
    let proto = ctor_obj.get(js_string!("prototype"), context)?;
    let proto_obj = proto.as_object();
    Ok(JsObject::from_proto_and_data(proto_obj, data))
}

pub fn obj_or_throw(this: &JsValue, name: &str) -> JsResult<JsObject> {
    this.as_object().ok_or_else(|| not_a(name))
}

// ---------------------------------------------------------------------
// Shared accessor-registration helpers, used by every host class.
// ---------------------------------------------------------------------

type NativeFn = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

pub fn accessor_ro(class: &mut boa_engine::class::ClassBuilder, name: &str, get: NativeFn) {
    accessor(class, name, get, None);
}

pub fn accessor_rw(class: &mut boa_engine::class::ClassBuilder, name: &str, get: NativeFn, set: NativeFn) {
    accessor(class, name, get, Some(set));
}

fn accessor(class: &mut boa_engine::class::ClassBuilder, name: &str, get: NativeFn, set: Option<NativeFn>) {
    let context = class.context();
    let getter = boa_engine::object::FunctionObjectBuilder::new(
        context.realm(),
        boa_engine::NativeFunction::from_fn_ptr(get),
    )
    .length(0)
    .build();
    let setter = set.map(|s| {
        boa_engine::object::FunctionObjectBuilder::new(
            context.realm(),
            boa_engine::NativeFunction::from_fn_ptr(s),
        )
        .length(1)
        .build()
    });
    let attribute = if setter.is_some() {
        boa_engine::property::Attribute::NON_ENUMERABLE
    } else {
        boa_engine::property::Attribute::READONLY | boa_engine::property::Attribute::NON_ENUMERABLE
    };
    class.accessor(js_string!(name), Some(getter), setter, attribute);
}

/// Opaque key identifying one open document in [`HostState`].
pub type DocKey = u32;

/// One open document: its editable engine plus the asset bytes that travel
/// with it (embedded/linked images etc.) — mirrors the
/// `(Document, AssetStore)` pairing `amalith_io::container::load` returns.
#[derive(Debug)]
pub struct OpenDocument {
    pub editor: Editor,
    pub assets: AssetStore,
    /// Absolute path the document was opened from / last saved to, if any
    /// (`Document.fullName` in ExtendScript; `None` for `app.documents.add()`).
    pub full_name: Option<PathBuf>,
    pub saved: bool,
    /// `Document.selection` — no such concept exists in `amalith-core`
    /// itself (it's a GUI notion there); backed here as plain state so a
    /// script (or a preamble run before it in the same `run_pipeline`
    /// call) can set it and a later script can read it back, matching how
    /// a real headless batch would have to stand in for "the user already
    /// selected something" — nothing populates this on its own.
    pub selection: Vec<amalith_core::ObjectId>,
}

/// Everything native host objects need to reach the real editing engine.
pub struct HostState {
    pub documents: HashMap<DocKey, OpenDocument>,
    pub active: Option<DocKey>,
    next_key: DocKey,
    pub cwd: PathBuf,
    /// `UserInteractionLevel.DONTDISPLAYALERTS` — suppresses `alert()` and
    /// auto-dismisses `ScriptUI` dialogs instead of blocking on stdin, since
    /// this shim always runs headless/unattended.
    pub non_interactive: bool,
    /// Lazily built on first `TextFrame.createOutline()` — loading system
    /// fonts is the expensive part, so one `Shaper` is reused for every
    /// outline call in a `run_pipeline` invocation rather than rebuilt per
    /// call.
    shaper: Option<crate::outline::Shaper>,
}

pub type SharedHost = Rc<RefCell<HostState>>;

impl std::fmt::Debug for HostState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostState")
            .field("documents", &self.documents.keys().collect::<Vec<_>>())
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl HostState {
    /// Lazily initialized: loading system fonts is the expensive part of
    /// building a `Shaper`, so it only happens on the first
    /// `TextFrame.createOutline()` call, not at startup.
    pub fn shaper(&mut self) -> &mut crate::outline::Shaper {
        self.shaper.get_or_insert_with(crate::outline::Shaper::new)
    }

    pub fn new(cwd: PathBuf) -> Self {
        Self {
            documents: HashMap::new(),
            active: None,
            next_key: 1,
            cwd,
            non_interactive: true,
            shaper: None,
        }
    }

    pub fn insert_document(
        &mut self,
        editor: Editor,
        assets: AssetStore,
        full_name: Option<PathBuf>,
    ) -> DocKey {
        let key = self.next_key;
        self.next_key += 1;
        self.documents.insert(
            key,
            OpenDocument {
                editor,
                assets,
                full_name,
                saved: true,
                selection: Vec::new(),
            },
        );
        self.active = Some(key);
        key
    }

    pub fn close_document(&mut self, key: DocKey) {
        self.documents.remove(&key);
        if self.active == Some(key) {
            self.active = self.documents.keys().next().copied();
        }
    }
}
