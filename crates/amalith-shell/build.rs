//! Build-time codegen:
//!
//! - Embeds the Windows app icon into `Amalith.exe` (no-op elsewhere).
//!   When cross-compiling to Windows, `embed-resource` needs an
//!   `llvm-rc` / `rc.exe` on PATH — `cargo xwin` provides one.
//! - Scans `assets/newdoc-art/` and writes `$OUT_DIR/cnd_art.rs` — a
//!   `CND_ART: &[&[u8]]` of `include_bytes!` for every `.png` in there.
//!   The folder's contents *are* the rotation: adding an export puts it in,
//!   deleting one takes it out, no source change. It lives under the crate
//!   (not `branding/`, which is regenerated design output) so the build
//!   can't be broken by a branding wipe. The `.ai` master sits alongside
//!   for reference and is ignored (not a `.png`). The embedded list is
//!   printed as a build warning so it's visible.

use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("windows/amalith.rc", embed_resource::NONE);
    }
    gen_cnd_art();
}

fn gen_cnd_art() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/newdoc-art");
    // Rebuild when the folder gains / loses a file...
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut pngs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|s| s.to_str()).map(str::to_ascii_lowercase)
                == Some("png".into())
        })
        .collect();
    pngs.sort();

    let names: Vec<String> = pngs
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
        .collect();
    println!(
        "cargo:warning=New Document art: {} image(s) — {}",
        names.len(),
        names.join(", ")
    );

    let mut body = String::from("pub const CND_ART: &[&[u8]] = &[\n");
    for p in &pngs {
        // ...and when any one of them changes.
        println!("cargo:rerun-if-changed={}", p.display());
        body.push_str(&format!("    include_bytes!(r\"{}\"),\n", p.display()));
    }
    body.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("cnd_art.rs");
    std::fs::write(&out, body).expect("write cnd_art.rs");
}
