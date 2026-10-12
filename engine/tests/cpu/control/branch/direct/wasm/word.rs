use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    cpu::{
        dbt::{ArtifactError, BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            decode::decode_one,
            ir::{BranchTarget, Operation},
        },
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
fn actual_word_jump_truncates_targets_and_preserves_live_state_in_six_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root.join("target/p2-word-jump-fixtures").join(format!(
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
    assert_eq!(ARENA_SIZE, 4612);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (i, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (i as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let rows = [
        (0x1000_u32, 0_u16, 0x1004),
        (0x1020, 0x7fff, 0x9023),
        (0x1040, 0x8000, 0x9044),
        (0x1060, 0xffff, 0x1063),
        (0x1080, 0xf066, 0x00ea),
        (0x10a0, 0x6766, 0x780a),
        (0x1234_fffc, 2, 2),
        (0xffff_fffc, 2, 2),
    ];
    let jumps: Vec<_> = rows
        .iter()
        .map(|&(pc, raw, target)| {
            (
                pc,
                vec![0x66, 0xe9, raw as u8, (raw >> 8) as u8],
                1,
                Some(target),
            )
        })
        .collect();
    let live = vec![
        (0x1400, vec![0x66, 0xe9, 0x1c, 0], 1, Some(0x1420)),
        (
            0x1420,
            vec![
                0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0x66, 0xe9, 0xc3, 0,
            ],
            4,
            None,
        ),
        (0x14f0, vec![0x66, 0xb9, 0x44, 0x33, 0xeb, 0], 2, None),
    ];
    let mut memory = AddressSpace::new(4).unwrap();
    for page in [0x1000, 0x1234_f000, 0xffff_f000, 0x7000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    for (name, blocks) in [("jumps", &jumps), ("live", &live)] {
        assert!(blocks.len() <= 8 && blocks.iter().map(|b| b.2).sum::<usize>() <= 64);
        for (i, (pc, bytes, _, target)) in blocks.iter().enumerate() {
            memory.write(GuestAddress(*pc), bytes).unwrap();
            fs::write(output.join(format!("{name}-{i}.x86")), bytes).unwrap();
            if let Some(target) = target {
                let decoded = decode_one(&memory, GuestAddress(*pc)).unwrap();
                assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(4)));
                assert_eq!(
                    decoded.operation(),
                    &Operation::Jump {
                        target: BranchTarget::Direct(GuestAddress(*target))
                    }
                );
            }
        }
    }
    for page in [0x1000, 0x1234_f000, 0xffff_f000] {
        memory
            .protect(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::EXECUTE,
            )
            .unwrap();
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x7000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let mut units = Vec::new();
    for (name, blocks) in [("jumps", &jumps), ("live", &live)] {
        let specs: Vec<_> = blocks
            .iter()
            .map(|(pc, bytes, _, _)| BlockSpec {
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
            entry: GuestAddress(0x1000),
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    assert!(probe.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1000), &[0x66]).unwrap();
    assert_eq!(
        probe.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
    fs::write(output.join("standalone-currentness.json"), b"{\"execute_only_units\":4,\"protection_rejects_retrieval\":true,\"same_byte_probe_units\":1,\"same_byte_write_rejects_retrieval\":true}").unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/branch/direct/fixtures/p2-word-jump/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "WORD jump integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
