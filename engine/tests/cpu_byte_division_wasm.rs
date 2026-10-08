use ring3_engine::{
    abi::arena::ARENA_SIZE,
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn standalone(output: &Path, name: &str, pc: u32, bytes: &[u8], specs: &[BlockSpec]) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
    for entries in [false, true] {
        let unit = if entries {
            compile_entry_region(
                &memory,
                &specs.iter().map(|s| s.entry).collect::<Vec<_>>(),
                CompileLimits::default(),
            )
            .unwrap()
        } else {
            compile_region(&memory, specs, CompileLimits::default()).unwrap()
        };
        fs::write(
            output.join(format!(
                "standalone-{}-{name}.wasm",
                if entries { "entries" } else { "extent" }
            )),
            unit.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn actual_byte_division_captures_ax_aliases_and_has_precise_retained_faults() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(
        engine.is_file(),
        "build the integrated actual engine Wasm first"
    );
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the actual engine SHA-256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let output = root.join("target/p2-byte-division-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 1).unwrap().arena(),
    )
    .unwrap();
    for (kind, base) in [("unsigned", 0xf0), ("signed", 0xf8)] {
        let mut bytes = vec![0xcc; 128];
        let specs: Vec<_> = (0..8)
            .map(|source| {
                let at = source * 16;
                bytes[at..at + 3].copy_from_slice(&[0xf6, base + source as u8, 0xe9]);
                bytes[at + 3..at + 7]
                    .copy_from_slice(&(0x1800_i32 - (0x1000 + at as i32 + 7)).to_le_bytes());
                BlockSpec {
                    entry: GuestAddress(0x1000 + at as u32),
                    byte_length: 7,
                }
            })
            .collect();
        assert_eq!(specs.len(), 8);
        standalone(&output, kind, 0x1000, &bytes, &specs);
    }
    let mut chain = vec![0xcc; 0x206];
    chain[..16].copy_from_slice(&[
        0xf6, 0xf0, 0xb4, 0xff, 0xb0, 0xfb, 0xf6, 0xfc, 0xf6, 0xf0, 0xb0, 0, 0xf6, 0xf0, 0xeb, 0,
    ]);
    chain[0x200..].copy_from_slice(&[0xb1, 0, 0xf6, 0xf1, 0xeb, 0]);
    standalone(
        &output,
        "chain",
        0x1400,
        &chain,
        &[
            BlockSpec {
                entry: GuestAddress(0x1400),
                byte_length: 16,
            },
            BlockSpec {
                entry: GuestAddress(0x1600),
                byte_length: 6,
            },
        ],
    );
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-division/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .arg(ARENA_SIZE.to_string())
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "byte division integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
