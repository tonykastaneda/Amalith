//! Scratch fixture generator for manually exercising the interpreter
//! against real, unmodified scripts from outside this repo. Not part of
//! the test suite — run with `cargo run -p amalith-script --example
//! make_fixture -- <out.amalith>`.

use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Affine, Document, Rect, TextData};

fn main() {
    let out = std::env::args().nth(1).expect("usage: make_fixture <out.amalith>");

    let mut editor = Editor::new(Document::new("Scratch"));
    editor
        .execute(Command::CreateArtboard { name: "Board A".into(), rect: Rect::new(0.0, 0.0, 800.0, 600.0), index: None })
        .unwrap();
    editor
        .execute(Command::CreateArtboard { name: "Board B".into(), rect: Rect::new(900.0, 0.0, 1700.0, 600.0), index: None })
        .unwrap();

    let layer_id = match editor.execute(Command::CreateLayer { name: "Layer 1".into(), index: None }).unwrap() {
        CommandOutcome::Layer(id) => id,
        other => panic!("{other:?}"),
    };

    editor
        .execute(Command::CreateRect { layer: layer_id, rect: Rect::new(10.0, 10.0, 110.0, 60.0), name: Some("box".into()) })
        .unwrap();

    let mut data = TextData { content: "Hello".into(), ..TextData::default() };
    data.style.family = "Helvetica".into();
    data.style.size = 24.0;
    editor
        .execute(Command::CreateText { layer: layer_id, data, transform: Affine::translate((10.0, 100.0)), name: Some("greeting".into()) })
        .unwrap();

    amalith_io::save(editor.document(), &amalith_io::AssetStore::new(), &out).unwrap();
    println!("wrote {out}");
}
