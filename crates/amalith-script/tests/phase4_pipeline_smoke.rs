//! Proves `run_pipeline`'s core multi-script claim: one `Context` is
//! reused across sequential script loads, so `$.global` set by script A is
//! visible to script B — the same mechanism `start.jsx` relies on to chain
//! RAGE's six real pipeline scripts together.

use amalith_script::run::run_pipeline_with_host;

#[test]
fn global_state_and_entry_points_persist_across_sequential_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let script_a = dir.path().join("a.jsx");
    let script_b = dir.path().join("b.jsx");

    std::fs::write(
        &script_a,
        r#"
        $.global.counter = 41;
        $.global.bumpAndGreet = function (name) {
            $.global.counter = $.global.counter + 1;
            return "hi " + name + " #" + $.global.counter;
        };
        "#,
    )
    .unwrap();

    std::fs::write(
        &script_b,
        r#"
        if (typeof $.global.bumpAndGreet !== "function") {
            throw new Error("entry point from script A was not visible in script B");
        }
        $.global.result = $.global.bumpAndGreet("RAGE");
        "#,
    )
    .unwrap();

    let host = run_pipeline_with_host(&[script_a, script_b]).unwrap();
    // The assertion that matters already happened inside script B (it
    // throws — and run_pipeline_with_host would return Err — if the
    // cross-script global wasn't visible). This just double-checks the
    // host is still alive/consistent afterward.
    assert!(host.borrow().documents.is_empty());
}
