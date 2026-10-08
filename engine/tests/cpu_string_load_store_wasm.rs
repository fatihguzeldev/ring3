use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bank() -> Vec<u8> {
    let mut bytes = vec![0xcc; 64];
    for (offset, code) in [
        (0, &[0xac, 0xeb, 0][..]),
        (8, &[0xaa, 0xeb, 0][..]),
        (16, &[0xac, 0xaa, 0xac, 0xaa, 0xeb, 0][..]),
        (32, &[0x90, 0xac, 0xeb, 0][..]),
        (40, &[0x90, 0xaa, 0xeb, 0][..]),
    ] {
        bytes[offset..offset + code.len()].copy_from_slice(code);
    }
    bytes
}

#[test]
fn actual_engine_lodsb_stosb_direction_faults_and_invalidation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let output = root
        .join("target/p2-string-load-store-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("string-load-store.x86"), bank()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-string-load-store/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "LODSB/STOSB integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
