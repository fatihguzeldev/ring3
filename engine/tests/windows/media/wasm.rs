#[path = "../../support/wasm.rs"]
mod support_wasm;

use std::{fs, path::Path, time::SystemTime};

#[test]
fn authored_pe_streams_audio_and_reads_input_through_event_waits() {
    run("engine/tests/windows/media/fixtures/runtime/run.mjs");
}

#[test]
fn sustained_authored_media_threads_storage_and_smc_share_one_bounded_instance() {
    run("engine/tests/windows/media/fixtures/runtime/lifecycle.mjs");
}

fn run(runner: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-media-wasm").join(format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(root, runner, Path::new(&engine), &output, Some(&expected));
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "Windows media Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
