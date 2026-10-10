use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
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

#[test]
fn actual_decimal_adjust_exhausts_inputs_and_consumes_live_flags_in_six_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root.join("target/p2-decimal-adjust-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(ARENA_SIZE, 4364);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (i, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (i as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let main = [
        (0x1000, vec![0x27, 0xeb, 0], 2),
        (0x1020, vec![0x2f, 0xeb, 0], 2),
    ];
    let live = [
        (
            0x1400,
            vec![
                0x00, 0xd8, 0x27, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0,
            ],
            6,
        ),
        (
            0x1440,
            vec![
                0x28, 0xd8, 0x2f, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0,
            ],
            6,
        ),
    ];
    let mut memory = AddressSpace::new(2).unwrap();
    for page in [0x1000, 0x7000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    for blocks in [&main[..], &live[..]] {
        for (pc, bytes, _) in blocks {
            memory.write(GuestAddress(*pc), bytes).unwrap();
        }
    }
    memory
        .write(GuestAddress(0x7010), &[0x55, 0xa5, 0x5a, 0xaa])
        .unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x7000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    for (name, blocks) in [("main", &main[..]), ("live", &live[..])] {
        assert!(blocks.len() <= 8 && blocks.iter().map(|b| b.2).sum::<usize>() <= 64);
        let specs: Vec<_> = blocks
            .iter()
            .map(|(pc, bytes, _)| BlockSpec {
                entry: GuestAddress(*pc),
                byte_length: bytes.len() as u32,
            })
            .collect();
        for entries in [false, true] {
            let unit = if entries {
                compile_entry_region(
                    &memory,
                    &specs.iter().map(|b| b.entry).collect::<Vec<_>>(),
                    CompileLimits::default(),
                )
            } else {
                compile_region(&memory, &specs, CompileLimits::default())
            }
            .unwrap();
            assert_eq!(unit.metadata().blocks, blocks.len());
            assert_eq!(
                unit.metadata().instructions,
                blocks.iter().map(|b| b.2).sum()
            );
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
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-decimal-adjust/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "decimal adjust integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
