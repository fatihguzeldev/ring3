use super::support_wasm as wasm_support;
use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn run_regression(name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "selected engine artifact must exist");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select its explicit SHA-256");
    let output = root.join("target/p2-data-wrap-regressions").join(format!(
        "{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 0x4457_5241)
        .unwrap()
        .arena()
        .to_vec();
    assert_eq!(initial.len(), 4796);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let script = format!(
        "engine/tests/cpu/segments/data/wrap/regressions/fixtures/p2-data-wrap-regressions/runtime/{name}.mjs"
    );
    let result = wasm_support::run_node(root, &script, &engine, &output, Some(&expected));
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "CPU data wrap {name} regression failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

#[test]
fn actual_wrapped_cmpxchg_fault_preserves_cpu_reply_and_exit_versions() {
    run_regression("compare-exit");
}

#[test]
fn actual_wrapped_xsave_request_zero_preserves_header_and_identity_write_currentness() {
    run_regression("xsave-header");
}

#[test]
fn actual_wrapped_pusha_popa_faults_validate_selected_slots_without_partial_publication() {
    run_regression("stack-fault");
}
