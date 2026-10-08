use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bank(opcode: u8, extension: u8) -> Vec<u8> {
    let forms: [Vec<u8>; 8] = [
        vec![0x0f, opcode, 0x0b],
        vec![0x0f, opcode, 0x00],
        vec![0x0f, opcode, 0x4c, 0x8a, 0x80],
        vec![0x0f, 0xba, (extension << 3) | 3, 0],
        vec![0x0f, 0xba, (extension << 3) | 3, 31],
        vec![0x0f, 0xba, (extension << 3) | 3, 32],
        vec![0x0f, 0xba, (extension << 3) | 3, 255],
        vec![0xb9, 32, 0, 0, 0, 0x0f, opcode, 0x0b],
    ];
    let mut bytes = vec![0xcc; 128];
    for (index, form) in forms.iter().enumerate() {
        let at = index * 16;
        bytes[at..at + form.len()].copy_from_slice(form);
        let end = at + form.len();
        bytes[end] = 0xe9;
        bytes[end + 1..end + 5]
            .copy_from_slice(&(0x1800_i32 - (0x1000 + end as i32 + 5)).to_le_bytes());
    }
    bytes
}

#[test]
fn actual_memory_bit_mutations_capture_old_bit_and_atomic_store() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let output = root
        .join("target/p2-memory-bit-mutation32-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    for (name, opcode, extension) in [("bts", 0xab, 5), ("btr", 0xb3, 6), ("btc", 0xbb, 7)] {
        fs::write(output.join(format!("{name}.x86")), bank(opcode, extension)).unwrap();
    }
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(ARENA_SIZE, 4364);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-bit-mutation32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory bit mutation integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
