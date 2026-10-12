use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn extended_records_refuse_malformed_and_aliased_inputs_before_publication() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    let output = root.join("target/p2-cpu-profile-boundaries").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result =
        Command::new("node")
            .arg(root.join(
                "engine/tests/cpu/profile/fixtures/p2-cpu-profile-boundaries/runtime/run.mjs",
            ))
            .arg(engine)
            .arg(&output)
            .output()
            .unwrap();
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
