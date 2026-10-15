use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_pause_and_compiler_polling_leaf_preserve_state_and_resume() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root.join("target/p2-pause-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(initial.len(), ARENA_SIZE);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let banks = [
        ("pause", 0x1000_u32, vec![0xf3, 0x90, 0xeb, 0], 2),
        (
            "live",
            0x1400,
            vec![
                0x04, 1, 0xf3, 0x90, 0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0,
            ],
            6,
        ),
    ];
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for (name, pc, bytes, _) in &banks {
        memory.write(GuestAddress(*pc), bytes).unwrap();
        fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    for (name, pc, bytes, instructions) in &banks {
        for entries in [false, true] {
            let unit = if entries {
                compile_entry_region(&memory, &[GuestAddress(*pc)], CompileLimits::default())
            } else {
                compile_region(
                    &memory,
                    &[BlockSpec {
                        entry: GuestAddress(*pc),
                        byte_length: bytes.len() as u32,
                    }],
                    CompileLimits::default(),
                )
            }
            .unwrap();
            assert_eq!(unit.metadata().blocks, 1);
            assert_eq!(unit.metadata().instructions, *instructions);
            assert!(unit.metadata().instructions <= 64);
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
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/hints/fixtures/p2-pause/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "pause integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
