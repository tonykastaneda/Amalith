//! `File`/`Folder` — the pure ExtendScript runtime filesystem API (not part
//! of the Illustrator DOM at all). RAGE's scripts use these for staging,
//! roster reads, and (via `system.callSystem`) file copies.

use std::path::{Path, PathBuf};

use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsArgs, JsData, JsNativeError, JsObject, JsResult, JsValue, NativeFunction};
use boa_gc::{Finalize, Trace};

use super::construct_instance;

fn resolve(cwd: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn arg_string(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    Ok(args
        .get_or_undefined(index)
        .to_string(context)?
        .to_std_string_escaped())
}

fn cwd(_context: &mut Context) -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

// ---------------------------------------------------------------------
// File
// ---------------------------------------------------------------------

#[derive(Debug)]
struct FileInner {
    path: PathBuf,
    encoding: String,
    mode: Option<char>,
    read_buf: Vec<u8>,
    read_pos: usize,
    write_buf: Vec<u8>,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsFile {
    #[unsafe_ignore_trace]
    inner: FileInner,
}

use super::not_a;

impl JsFile {
    /// Build a `JsFile` for `path` directly from Rust, bypassing the JS
    /// constructor's argument parsing — used wherever native code needs to
    /// hand a script a `File` (e.g. `Document.fullName`).
    pub(crate) fn for_path(path: PathBuf) -> Self {
        JsFile {
            inner: FileInner {
                path,
                encoding: "UTF-8".to_string(),
                mode: None,
                read_buf: Vec::new(),
                read_pos: 0,
                write_buf: Vec::new(),
            },
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.inner.path
    }

    fn obj(this: &JsValue) -> JsResult<JsObject> {
        this.as_object().ok_or_else(|| not_a("File"))
    }

    fn fs_name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
        Ok(JsValue::from(js_string!(f.inner.path.to_string_lossy().to_string())))
    }

    fn name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
        let name = f
            .inner
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(JsValue::from(js_string!(name)))
    }

    fn exists(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
        Ok(JsValue::from(f.inner.path.is_file()))
    }

    fn parent(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let path = {
            let obj = Self::obj(this)?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
            f.inner.path.clone()
        };
        match path.parent() {
            Some(p) => {
                let obj = construct_instance(context, "Folder", JsFolder::new_inner(p.to_path_buf()))?;
                Ok(JsValue::from(obj))
            }
            None => Ok(JsValue::null()),
        }
    }

    fn get_encoding(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
        Ok(JsValue::from(js_string!(f.inner.encoding.clone())))
    }

    fn set_encoding(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let enc = arg_string(args, 0, context)?;
        let obj = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("not a File")
        })?;
        let mut f = obj
            .downcast_mut::<Self>()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        f.inner.encoding = enc.to_uppercase();
        Ok(JsValue::undefined())
    }

    fn open(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let mode = arg_string(args, 0, context)?;
        let mode_char = mode.chars().next().unwrap_or('r');
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        let mut f = obj
            .downcast_mut::<Self>()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        if mode_char == 'r' {
            f.inner.read_buf = std::fs::read(&f.inner.path).unwrap_or_default();
            f.inner.read_pos = 0;
        } else {
            f.inner.write_buf.clear();
        }
        f.inner.mode = Some(mode_char);
        Ok(JsValue::from(true))
    }

    fn read(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        let mut f = obj
            .downcast_mut::<Self>()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        let remaining = f.inner.read_buf[f.inner.read_pos..].to_vec();
        f.inner.read_pos = f.inner.read_buf.len();
        let text = if f.inner.encoding == "BINARY" {
            remaining.iter().map(|&b| b as char).collect::<String>()
        } else {
            String::from_utf8_lossy(&remaining).into_owned()
        };
        Ok(JsValue::from(js_string!(text)))
    }

    fn write(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let text = arg_string(args, 0, context)?;
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        let mut f = obj
            .downcast_mut::<Self>()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        if f.inner.encoding == "BINARY" {
            f.inner.write_buf.extend(text.chars().map(|c| c as u8));
        } else {
            f.inner.write_buf.extend(text.as_bytes());
        }
        Ok(JsValue::from(true))
    }

    fn close(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        let mut f = obj
            .downcast_mut::<Self>()
            .ok_or_else(|| JsNativeError::typ().with_message("not a File"))?;
        if f.inner.mode == Some('w') {
            let _ = std::fs::write(&f.inner.path, &f.inner.write_buf);
        }
        f.inner.mode = None;
        Ok(JsValue::from(true))
    }

    fn remove(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("File"))?;
        let ok = std::fs::remove_file(&f.inner.path).is_ok();
        Ok(JsValue::from(ok))
    }
}

impl Class for JsFile {
    const NAME: &'static str = "File";
    const LENGTH: usize = 1;

    fn data_constructor(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let path = arg_string(args, 0, context)?;
        let cwd = cwd(context);
        Ok(JsFile {
            inner: FileInner {
                path: resolve(&cwd, &path),
                encoding: "UTF-8".to_string(),
                mode: None,
                read_buf: Vec::new(),
                read_pos: 0,
                write_buf: Vec::new(),
            },
        })
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(js_string!("open"), 1, NativeFunction::from_fn_ptr(Self::open));
        class.method(js_string!("read"), 0, NativeFunction::from_fn_ptr(Self::read));
        class.method(js_string!("write"), 1, NativeFunction::from_fn_ptr(Self::write));
        class.method(js_string!("close"), 0, NativeFunction::from_fn_ptr(Self::close));
        class.method(js_string!("remove"), 0, NativeFunction::from_fn_ptr(Self::remove));
        accessor(class, "fsName", Self::fs_name, None);
        accessor(class, "name", Self::name, None);
        accessor(class, "exists", Self::exists, None);
        accessor(class, "parent", Self::parent, None);
        accessor_rw(class, "encoding", Self::get_encoding, Self::set_encoding);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Folder
// ---------------------------------------------------------------------

#[derive(Debug)]
struct FolderInner {
    path: PathBuf,
}

#[derive(Debug, Trace, Finalize, JsData)]
pub struct JsFolder {
    #[unsafe_ignore_trace]
    inner: FolderInner,
}

impl JsFolder {
    pub(crate) fn new_inner(path: PathBuf) -> Self {
        JsFolder {
            inner: FolderInner { path },
        }
    }

    fn obj(this: &JsValue) -> JsResult<JsObject> {
        this.as_object().ok_or_else(|| not_a("Folder"))
    }

    fn fs_name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
        Ok(JsValue::from(js_string!(f.inner.path.to_string_lossy().to_string())))
    }

    fn name(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
        let name = f
            .inner
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(JsValue::from(js_string!(name)))
    }

    fn exists(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
        Ok(JsValue::from(f.inner.path.is_dir()))
    }

    fn parent(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let path = {
            let obj = Self::obj(this)?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
            f.inner.path.clone()
        };
        match path.parent() {
            Some(p) => {
                let obj = construct_instance(context, "Folder", JsFolder::new_inner(p.to_path_buf()))?;
                Ok(JsValue::from(obj))
            }
            None => Ok(JsValue::null()),
        }
    }

    fn create(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        let obj = Self::obj(this)?;
        let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
        let ok = std::fs::create_dir_all(&f.inner.path).is_ok();
        Ok(JsValue::from(ok))
    }

    fn get_files(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let pattern = if args.is_empty() {
            "*".to_string()
        } else {
            arg_string(args, 0, context)?
        };
        let path = {
            let obj = Self::obj(this)?;
            let f = obj.downcast_ref::<Self>().ok_or_else(|| not_a("Folder"))?;
            f.inner.path.clone()
        };
        let suffix = pattern.trim_start_matches('*');
        let mut matches: Vec<PathBuf> = std::fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && (suffix.is_empty()
                        || p.file_name()
                            .map(|n| n.to_string_lossy().ends_with(suffix))
                            .unwrap_or(false))
            })
            .collect();
        matches.sort();

        let mut values = Vec::with_capacity(matches.len());
        for p in matches {
            let obj = construct_instance(context, "File", JsFile::for_path(p))?;
            values.push(JsValue::from(obj));
        }
        let array = JsArray::from_iter(values, context);
        Ok(JsValue::from(array))
    }
}

impl Class for JsFolder {
    const NAME: &'static str = "Folder";
    const LENGTH: usize = 1;

    fn data_constructor(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let path = arg_string(args, 0, context)?;
        let cwd = cwd(context);
        Ok(JsFolder::new_inner(resolve(&cwd, &path)))
    }

    fn init(class: &mut ClassBuilder) -> JsResult<()> {
        class.method(js_string!("create"), 0, NativeFunction::from_fn_ptr(Self::create));
        class.method(
            js_string!("getFiles"),
            1,
            NativeFunction::from_fn_ptr(Self::get_files),
        );
        accessor(class, "fsName", Self::fs_name, None);
        accessor(class, "name", Self::name, None);
        accessor(class, "exists", Self::exists, None);
        accessor(class, "parent", Self::parent, None);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// accessor helpers
// ---------------------------------------------------------------------

type NativeFn = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

fn accessor(class: &mut ClassBuilder, name: &str, get: NativeFn, set: Option<NativeFn>) {
    let context = class.context();
    let getter = boa_engine::object::FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(get),
    )
    .length(0)
    .build();
    let setter = set.map(|s| {
        boa_engine::object::FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(s))
            .length(1)
            .build()
    });
    class.accessor(
        js_string!(name),
        Some(getter),
        setter,
        Attribute::READONLY | Attribute::NON_ENUMERABLE,
    );
}

fn accessor_rw(class: &mut ClassBuilder, name: &str, get: NativeFn, set: NativeFn) {
    let context = class.context();
    let getter = boa_engine::object::FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(get),
    )
    .length(0)
    .build();
    let setter = boa_engine::object::FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(set),
    )
    .length(1)
    .build();
    class.accessor(
        js_string!(name),
        Some(getter),
        Some(setter),
        Attribute::NON_ENUMERABLE,
    );
}

pub fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsFile>()?;
    context.register_global_class::<JsFolder>()?;

    // Folder.desktop — used only as a last-resort fallback in every RAGE
    // script's getScriptFolder(); our CLI always sets $.fileName, so this
    // path is rarely hit, but is implemented for completeness.
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let desktop = PathBuf::from(home).join("Desktop");
    let desktop_obj = construct_instance(context, "Folder", JsFolder::new_inner(desktop))?;
    let ctor = context.global_object().get(js_string!("Folder"), context)?;
    if let Some(ctor_obj) = ctor.as_object() {
        ctor_obj.set(js_string!("desktop"), JsValue::from(desktop_obj), false, context)?;
    }

    Ok(())
}
