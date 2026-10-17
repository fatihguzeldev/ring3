#[path = "../../support/wasm.rs"]
mod support_wasm;

#[test]
fn authored_foundations_share_real_generated_wasm_dispatch_and_lifetime() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = support_wasm::selected_engine(root);
    let output = root
        .join("target/windows-foundations-wasm")
        .join(format!("run-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    let expected = std::env::var("RING3_ENGINE_SHA256").unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/windows/foundations/fixtures/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    std::fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    std::fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
