use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn shared_javascript_fixture_defaults_match_the_current_engine_arena() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = root.join("target/fixture-arena-defaults").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = output.join("initial-arena.bin");
    fs::write(&initial, EngineInstance::new(1, 7).unwrap().arena()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/support/arena.test.mjs"))
        .arg(initial)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture arena defaults differ:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
