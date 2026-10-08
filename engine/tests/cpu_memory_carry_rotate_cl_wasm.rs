use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{abi::arena::TRANSFER_OFFSET, cpu::dbt::CompileLimits, process::EngineInstance};

const PC: u32 = 0x1000;
const HIGH: u32 = 0xc841_1000;

fn bank(extension: u8) -> (Vec<u8>, Vec<Vec<u32>>) {
    let mut bytes = vec![0xcc; 4096];
    let mut at = 0usize;
    let mut groups = Vec::new();
    let mut append = |form: Vec<u8>, canary: bool| {
        let entry = PC + u32::try_from(at).unwrap();
        let mut code = form;
        code.extend_from_slice(&[0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2]);
        if canary {
            code.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
        }
        code.push(0xe9);
        let next = i32::try_from(at + code.len() + 4).unwrap();
        code.extend_from_slice(&(0xf00_i32 - next).to_le_bytes());
        bytes[at..at + code.len()].copy_from_slice(&code);
        at += code.len();
        entry
    };
    groups.push(vec![append(
        vec![0xd3, 5 | extension, 0x10, 0x40, 0, 0],
        false,
    )]);
    let forms = [
        vec![0xd3, extension],
        vec![0xd3, 0x41 | extension, 0xf0],
        vec![0xd3, 0x44 | extension, 0x4a, 0x11],
        vec![0xd3, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff],
        vec![0xd3, 0x04 | extension, 0x24],
        vec![0xd3, 0x45 | extension, 0],
        vec![0xd3, 0x06 | extension],
        vec![0xd3, 0x87 | extension, 0, 1, 0, 0],
    ];
    for (index, form) in forms.iter().enumerate() {
        append(form.clone(), index == 7);
    }
    append(vec![0x8d, 0x5b, 1, 0xd3, 0x07 | extension], true);
    for (raw, producer, form) in [(1, 0xf8, 1_usize), (0, 0xf9, 2), (17, 0xf5, 4)] {
        let mut code = vec![0xb1, raw, producer];
        code.extend_from_slice(&forms[form]);
        code.extend_from_slice(&forms[form]);
        append(code, true);
    }
    assert_eq!(at, 238);
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [1]);
    for slot in 0..3 {
        let at = 0xe80 + slot * 20;
        let mut code = vec![
            0xbe,
            1,
            0,
            0,
            0x80,
            0xd3,
            0x07 | extension,
            0x0f,
            0x92,
            0xc0,
            0x0f,
            0x90,
            0xc2,
            0xbb,
            0xef,
            0xbe,
            0xad,
            0xde,
            0xeb,
        ];
        code.push(u8::try_from(0xf00 - at - 20).unwrap());
        assert_eq!(code.len(), 20);
        bytes[at..at + code.len()].copy_from_slice(&code);
    }
    bytes[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    (bytes, groups)
}

#[test]
fn native_bound_and_actual_engine_memory_cl_dword_carry_rotates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine_path = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(
        engine_path.is_file(),
        "build the current engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root
        .join("target/p2-memory-carry-rotate-cl32-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut manifest = Vec::new();
    for (kind, extension, low) in [("left", 0x10, 1_u32), ("right", 0x18, 3)] {
        let (bytes, groups) = bank(extension);
        fs::write(output.join(format!("memory-{kind}.x86")), &bytes).unwrap();
        let key = (u64::from(HIGH) << 32) | u64::from(low);
        let mut engine = EngineInstance::new(1, key).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(&bytes);
        engine.upload(PC, bytes.len() as u32).unwrap();
        for (group, entries) in groups.iter().enumerate() {
            for (index, entry) in entries.iter().enumerate() {
                let at = TRANSFER_OFFSET + index * 4;
                engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&entry.to_le_bytes());
            }
            let generation = engine.compile_entries(entries.len() as u32, 0).unwrap();
            assert_eq!(generation, (group + 1) as u32);
            assert_eq!(engine.generation(), generation);
            engine.guard(key, generation).unwrap();
            let module = engine.artifact_bytes().unwrap();
            assert!(module.len() <= limits.wasm_bytes);
            let file = format!("native-{kind}-g{group}.wasm");
            fs::write(output.join(&file), module).unwrap();
            manifest.push(format!(
                "{{\"kind\":\"{kind}\",\"group\":{group},\"key_low\":{low},\"key_high\":{HIGH},\"generation\":{generation},\"entries\":{entries:?},\"file\":\"{file}\"}}"
            ));
        }
    }
    fs::write(
        output.join("native-manifest.json"),
        format!("[{}]", manifest.join(",")),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-carry-rotate-cl32/run.mjs"))
        .arg(&engine_path)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory dword carry cl integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
