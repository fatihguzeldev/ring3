use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, X87_OFFSET},
        x86::encode_x87,
    },
    cpu::x86::X87State,
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bank() -> Vec<u8> {
    let mut bytes = vec![0xcc; 64];
    for (at, code) in [
        (0, &[0x66, 0xad, 0xeb, 0][..]),
        (16, &[0xbe, 8, 0x70, 0, 0, 0x66, 0xad, 0xeb, 0][..]),
        (32, &[0x66, 0xad, 0x66, 0xad, 0xeb, 0][..]),
    ] {
        bytes[at..at + code.len()].copy_from_slice(code);
    }
    bytes
}

#[test]
fn actual_engine_lodsw_direction_faults_and_retained_ram_repair() {
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
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let output = root
        .join("target/p2-word-load-string-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("lodsw.x86"), bank()).unwrap();
    let mut initial = EngineInstance::new(1, 1).unwrap().arena().to_vec();
    assert_eq!(initial.len(), ARENA_SIZE);
    let fp = X87State {
        control: 0x027f,
        status: 0x81a5,
        registers: std::array::from_fn(|i| std::array::from_fn(|j| ((i * 10 + j) * 29 + 17) as u8)),
        ..X87State::default()
    };
    encode_x87(&fp, &mut initial[X87_OFFSET..]).unwrap();
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-load-string/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "LODSW integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
