use ring3_engine::{
    abi::{arena::ARENA_SIZE, x86::encode_x87},
    cpu::x86::X87State,
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_memory_bt_signed_index_immediate_mask_and_precise_retry() {
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
        .join("target/p2-memory-bit-test32-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 1).unwrap().arena(),
    )
    .unwrap();
    let fp = X87State {
        control: 0x127f,
        status: 0xa581,
        tag: 0x59af,
        opcode: 0x5a3,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x89ab_cdef,
        code_selector: 0x1234,
        data_selector: 0x5678,
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 37 + b * 13 + 11) as u8)),
    };
    let mut tail = [0_u8; 128];
    encode_x87(&fp, &mut tail).unwrap();
    fs::write(output.join("opaque-fp.bin"), tail).unwrap();
    let forms: [&[u8]; 8] = [
        &[0x0f, 0xa3, 0x0b],
        &[0x0f, 0xa3, 0x00],
        &[0x0f, 0xa3, 0x4c, 0x8a, 0x80],
        &[0x0f, 0xba, 0x23, 0],
        &[0x0f, 0xba, 0x23, 31],
        &[0x0f, 0xba, 0x23, 32],
        &[0x0f, 0xba, 0x23, 255],
        &[0xb9, 32, 0, 0, 0, 0x0f, 0xa3, 0x0b],
    ];
    let mut bytes = vec![0xcc; 128];
    for (i, form) in forms.iter().enumerate() {
        let at = i * 16;
        bytes[at..at + form.len()].copy_from_slice(form);
        let end = at + form.len();
        bytes[end] = 0xe9;
        bytes[end + 1..end + 5]
            .copy_from_slice(&(0x1800_i32 - (0x1000 + end as i32 + 5)).to_le_bytes());
    }
    fs::write(output.join("bt.x86"), bytes).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-bit-test32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .arg(ARENA_SIZE.to_string())
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory BT integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
