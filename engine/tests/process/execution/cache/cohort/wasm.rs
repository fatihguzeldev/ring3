use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_warm_cohort_preserves_faults_pins_cancellation_and_complete_owner_coverage() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let output = root
        .join("artifacts/execution-cohort-fixtures")
        .join(format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    let result = crate::support_wasm::run_node(
        root,
        "engine/tests/process/execution/cache/cohort/fixtures/run.mjs",
        &engine,
        &output,
        std::env::var("RING3_ENGINE_SHA256").ok().as_deref(),
    );
    assert!(
        result.status.success(),
        "actual warm cohort controls failed (output: {}):\n{}\n{}",
        output.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    fs::remove_dir_all(&output).expect("remove successful execution cohort fixture output");
}
