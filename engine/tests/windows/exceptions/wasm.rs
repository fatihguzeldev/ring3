#[path = "../../support/wasm.rs"]
mod support_wasm;
use std::{fs, path::Path, time::SystemTime};
#[test]
fn authored_pe_exceptions_and_cpu_faults_execute_guest_handlers_in_wasm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-exceptions-wasm").join(format!(
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
        "engine/tests/windows/exceptions/fixtures/runtime/run.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "exception/CPU Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

#[test]
fn authored_pe_i386_nested_and_collided_unwind_execute_guest_handler_dispositions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-exceptions-wasm").join(format!(
        "unwind-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/windows/exceptions/fixtures/runtime/unwind.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "i386 unwind Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn compiler_produced_cpp_with_pinned_open_source_crt_executes_hosted_lifecycle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let artifacts = std::env::var("RING3_OSS_CRT_FIXTURE_DIR")
        .expect("build and select the pinned open-source CRT fixture directory");
    assert!(Path::new(&artifacts).join("provenance.json").is_file());
    let output = root.join("target/windows-exceptions-wasm").join(format!(
        "hosted-crt-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/windows/exceptions/fixtures/crt/runtime/run.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "hosted compiler/CRT Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
