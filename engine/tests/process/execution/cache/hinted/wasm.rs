use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_hint_guided_reoptimization_preserves_whole_owner_frontiers() {
    run_fixture("run.mjs");
}

#[test]
fn actual_hint_guided_reoptimization_preserves_exact_x87_frontiers() {
    run_fixture("run-fp.mjs");
}

fn run_fixture(runner: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let output = root.join("target/execution-hinted-fixtures").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&output).unwrap();
    let result = crate::support_wasm::run_node(
        root,
        &format!("engine/tests/process/execution/cache/hinted/fixtures/{runner}"),
        &engine,
        &output,
        std::env::var("RING3_ENGINE_SHA256").ok().as_deref(),
    );
    assert!(
        result.status.success(),
        "actual hinted reoptimization ({runner}) failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
