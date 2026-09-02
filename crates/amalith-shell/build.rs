//! Embeds the Windows app icon into `Amalith.exe`. A no-op on every other
//! platform. When cross-compiling to Windows, `embed-resource` needs an
//! `llvm-rc` / `rc.exe` on PATH — `cargo xwin` provides one.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("windows/amalith.rc", embed_resource::NONE);
    }
}
