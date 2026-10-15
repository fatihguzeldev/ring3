#[path = "../../support/wasm.rs"]
mod support_wasm;

use std::{fs, path::Path, time::SystemTime};

#[test]
fn authored_pe_reads_maps_and_saves_files_through_async_host_progress() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-files-wasm").join(format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/windows/files/fixtures/runtime/run.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "Windows file Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
