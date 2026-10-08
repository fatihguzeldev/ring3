use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn banks() -> [Vec<u8>; 3] {
    let mut core = vec![0xcc; 64];
    let mut endpoints = vec![0xcc; 32];
    for (index, opcode) in (0xe0..=0xe3).enumerate() {
        core[index * 8..index * 8 + 2].copy_from_slice(&[opcode, 4]);
        core[32 + index * 8..34 + index * 8].copy_from_slice(&[opcode, 0xfe]);
        endpoints[index * 8..index * 8 + 2].copy_from_slice(&[opcode, 0x80]);
        endpoints[index * 8 + 4..index * 8 + 6].copy_from_slice(&[opcode, 0x7f]);
    }
    [core, endpoints, vec![0xe2, 0x80]]
}

#[test]
fn actual_count_branches_live_ecx_flags_cold_and_retained_loops() {
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
    let output = root.join("target/p2-count-branches-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    for (name, bank) in ["count-branches.x86", "endpoints.x86", "wrap.x86"]
        .into_iter()
        .zip(banks())
    {
        fs::write(output.join(name), bank).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-count-branches/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "count-branch fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
