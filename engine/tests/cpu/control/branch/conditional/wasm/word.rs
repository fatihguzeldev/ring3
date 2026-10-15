use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    cpu::dbt::{ArtifactError, BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_conditions_select_truncated_taken_and_full_false_pc_in_six_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root
        .join("target/p2-word-conditional-jump-fixtures")
        .join(format!(
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
    assert_eq!(initial.len(), ARENA_SIZE);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (i, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (i as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let displacements = [
        0_u16, 1, 0xffff, 0x8000, 0x7fff, 0x6666, 0x6767, 0xf0f0, 0xf2f2, 0xf3f3, 0x66f0, 0xf066,
        2, 0xfffe, 0x8001, 0x7ffe,
    ];
    let conditions: Vec<_> = displacements
        .iter()
        .enumerate()
        .map(|(cc, raw)| {
            (
                0x1234_1000 + cc as u32 * 0x20,
                vec![0x66, 0x0f, 0x80 + cc as u8, *raw as u8, (*raw >> 8) as u8],
                1_usize,
            )
        })
        .collect();
    let boundary_live = [
        (0x1234_fffb, vec![0x66, 0x0f, 0x84, 2, 0], 1),
        (0xffff_fffb, vec![0x66, 0x0f, 0x84, 2, 0], 1),
        (
            0x1234_1a00,
            vec![0x66, 0x39, 0xc8, 0x66, 0x0f, 0x84, 0xf8, 0xfd],
            2,
        ),
        (
            0x1234_1a08,
            vec![0x66, 0xba, 2, 0, 0x66, 0xe9, 0xf0, 0xfe],
            2,
        ),
        (0x1800, vec![0x66, 0xba, 1, 0, 0x66, 0xe9, 0xf8, 0], 2),
        (
            0x1900,
            vec![
                0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x94, 0xc7, 0xeb, 0,
            ],
            4,
        ),
    ];
    let banks = [
        ("conditions-0", &conditions[..8]),
        ("conditions-1", &conditions[8..]),
        ("boundary-live", &boundary_live[..]),
    ];
    let mut memory = AddressSpace::new(5).unwrap();
    for page in [0x1000, 0x1234_1000, 0x1234_f000, 0xffff_f000, 0x7000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    for (name, blocks) in banks {
        assert!(blocks.len() <= 8 && blocks.iter().map(|b| b.2).sum::<usize>() <= 64);
        assert!(blocks.iter().map(|b| b.1.len()).sum::<usize>() <= 65536);
        for (i, (pc, bytes, _)) in blocks.iter().enumerate() {
            memory.write(GuestAddress(*pc), bytes).unwrap();
            fs::write(output.join(format!("{name}-{i}.x86")), bytes).unwrap();
        }
    }
    for page in [0x1000, 0x1234_1000, 0x1234_f000, 0xffff_f000] {
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
    for (name, blocks) in banks {
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
                blocks.iter().map(|b| b.2).sum::<usize>()
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
            PageRange::new(GuestAddress(0x1234_1000), 1).unwrap(),
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
            entry: GuestAddress(0x1234_1000),
            byte_length: 5,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    assert!(probe.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1234_1000), &[0x66]).unwrap();
    assert_eq!(
        probe.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
    fs::write(output.join("standalone-currentness.json"), b"{\"execute_only_units\":6,\"protection_rejects_retrieval\":true,\"same_byte_probe_units\":1,\"same_byte_write_rejects_retrieval\":true}").unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/branch/conditional/fixtures/p2-word-conditional-jump/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "WORD conditional jump integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
