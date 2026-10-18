use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_owns_dispatch_terminal_receipts_and_preserves_detector_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let output = root.join("target/execution-receipt-fixtures").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = crate::support_wasm::run_node(
        root,
        "engine/tests/process/execution/receipt/fixtures/run.mjs",
        &engine,
        &output,
        std::env::var("RING3_ENGINE_SHA256").ok().as_deref(),
    );
    assert!(
        result.status.success(),
        "actual execution receipt fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
