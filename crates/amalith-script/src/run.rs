//! Runs one or more `.jsx` files sequentially against a single boa
//! `Context`/`HostState`, so a shared `$.global` (and every open document)
//! persists across script boundaries — matching how `start.jsx` chains
//! RAGE's six real pipeline scripts.

use std::path::Path;

use crate::engine;
use crate::host::SharedHost;

pub fn run_pipeline(paths: &[impl AsRef<Path>]) -> Result<(), String> {
    run_pipeline_with_host(paths).map(|_| ())
}

/// Same as [`run_pipeline`], but also hands back the [`SharedHost`] so
/// callers (tests, mainly) can inspect the resulting document state
/// without going back through JS.
pub fn run_pipeline_with_host(paths: &[impl AsRef<Path>]) -> Result<SharedHost, String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (mut context, host) = engine::new_context(cwd).map_err(|e| e.to_string())?;

    for path in paths {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;

        engine::set_current_file(&mut context, &path.to_string_lossy()).map_err(|e| e.to_string())?;

        engine::eval_source(&mut context, &src)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
    }

    Ok(host)
}
