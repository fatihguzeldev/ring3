use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    cpu::{
        dbt::{ArtifactError, BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{decode::decode_one, ir::Operation},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_canonical_alignment_nops_preserve_state_in_six_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root.join("target/p2-alignment-nop-fixtures").join(format!(
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
    let nops = [
        (0x1000_u32, vec![0x66, 0x0f, 0x1f, 0x44, 0, 0]),
        (0x1020, vec![0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]),
        (0x1040, vec![0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]),
    ];
    let blocks: Vec<_> = nops
        .iter()
        .map(|(pc, bytes)| {
            let mut bytes = bytes.clone();
            bytes.extend_from_slice(&[0xeb, 0]);
            (*pc, bytes)
        })
        .collect();
    let live = vec![(
        0x1400,
        vec![
            0x04, 1, 0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0, 0x66, 0x11, 0xd0, 0x0f, 0x92,
            0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0,
        ],
    )];
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for (name, bank) in [("nops", &blocks), ("live", &live)] {
        assert!(bank.len() <= 8);
        for (index, (pc, bytes)) in bank.iter().enumerate() {
            memory.write(GuestAddress(*pc), bytes).unwrap();
            fs::write(output.join(format!("{name}-{index}.x86")), bytes).unwrap();
        }
    }
    for (pc, bytes) in &nops {
        let instruction = decode_one(&memory, GuestAddress(*pc)).unwrap();
        assert_eq!(instruction.operation(), &Operation::Nop);
        assert_eq!(
            instruction.next_pc(),
            GuestAddress(*pc + bytes.len() as u32)
        );
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    let mut units = Vec::new();
    for (name, bank) in [("nops", &blocks), ("live", &live)] {
        let specs: Vec<_> = bank
            .iter()
            .map(|(pc, bytes)| BlockSpec {
                entry: GuestAddress(*pc),
                byte_length: bytes.len() as u32,
            })
            .collect();
        for entries in [false, true] {
            let unit = if entries {
                compile_entry_region(
                    &memory,
                    &specs.iter().map(|spec| spec.entry).collect::<Vec<_>>(),
                    CompileLimits::default(),
                )
            } else {
                compile_region(&memory, &specs, CompileLimits::default())
            }
            .unwrap();
            assert_eq!(unit.metadata().blocks, bank.len());
            assert_eq!(unit.metadata().instructions, 6);
            assert!(unit.metadata().instructions <= 64);
            fs::write(
                output.join(format!(
                    "standalone-{}-{name}.wasm",
                    if entries { "entries" } else { "extent" }
                )),
                unit.wasm_bytes(&memory).unwrap(),
            )
            .unwrap();
            units.push(unit);
        }
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    assert!(
        units
            .iter()
            .all(|unit| unit.wasm_bytes(&memory) == Err(ArtifactError::CodeInvalidated))
    );
    let probe = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1040),
            byte_length: 10,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    assert!(probe.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1049), &[0]).unwrap();
    assert_eq!(
        probe.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
    fs::write(output.join("standalone-currentness.json"), b"{\"execute_only_units\":4,\"protection_rejects_retrieval\":true,\"same_byte_final_displacement_rejects_retrieval\":true}").unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/hints/fixtures/p2-alignment-nop/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node-stdout.txt"), &result.stdout).unwrap();
    fs::write(output.join("node-stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "alignment nop integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
