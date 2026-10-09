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
        (0, &[0x66, 0xa7, 0xeb, 0][..]),
        (8, &[0x66, 0xaf, 0xeb, 0][..]),
        (
            16,
            &[
                0xb8, 0xff, 0x7f, 0x81, 0xa5, 0x66, 0xaf, 0xb8, 0, 0x80, 0x81, 0xa5, 0x66, 0xaf,
                0xeb, 6,
            ][..],
        ),
        (32, &[0x66, 0xa7, 0x66, 0xa7, 0xeb, 0][..]),
        (40, &[0x90, 0x66, 0xa7, 0xeb, 0][..]),
        (48, &[0x90, 0x66, 0xaf, 0xeb, 0][..]),
    ] {
        bytes[at..at + code.len()].copy_from_slice(code);
    }
    bytes
}

#[test]
fn actual_engine_word_string_comparisons_flags_and_retained_current_inputs() {
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
    let parent = root.join("target/p2-word-string-compare-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&output).unwrap();
    fs::write(output.join("word-string-compare.x86"), bank()).unwrap();
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
        .arg(root.join("engine/tests/fixtures/p2-word-string-compare/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "word string comparison integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
