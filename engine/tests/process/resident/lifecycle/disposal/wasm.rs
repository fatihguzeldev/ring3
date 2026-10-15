use std::{
    env, fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_wasm_discards_three_current_unacknowledged_units_and_reuses_capacity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root
        .join("target/p2-unacknowledged-disposal-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let copied_engine = output.join("engine.wasm");
    fs::copy(&engine, &copied_engine).unwrap();
    fs::write(
        output.join("engine-input-path.txt"),
        engine.to_string_lossy().as_bytes(),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join(
            "engine/tests/process/resident/lifecycle/fixtures/p2-unacknowledged-disposal/runtime/run.mjs",
        ))
        .arg(&copied_engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual unacknowledged disposal failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
