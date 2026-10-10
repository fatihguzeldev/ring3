use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, compile_entry_region, compile_region,
};
use ring3_engine::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn memory_with_code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = (pc as u64 - first as u64 + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 2).unwrap();
    memory
        .map_zeroed(range(first, pages), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

fn block(entry: u32, byte_length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(entry),
        byte_length,
    }
}

#[test]
fn generated_artifact_metadata_has_explicit_versions_and_counts() {
    let bytes = [
        0xb8, 0, 0, 0, 0, 0x83, 0xf9, 0, 0x74, 7, 0x01, 0xc8, 0x83, 0xe9, 1, 0xeb, 0xf4,
    ];
    let memory = memory_with_code(0x1000, &bytes);
    let compiled = compile_region(
        &memory,
        &[block(0x1000, 5), block(0x1005, 5), block(0x100a, 7)],
        CompileLimits::default(),
    )
    .unwrap();
    let metadata = compiled.metadata();
    assert_eq!(metadata.backend_version, 1);
    assert_eq!(metadata.abi_version, 1);
    assert_eq!(metadata.profile, 1);
    assert_eq!(metadata.blocks, 3);
    assert_eq!(metadata.instructions, 6);
    assert!(
        compiled
            .wasm_bytes(&memory)
            .unwrap()
            .starts_with(b"\0asm\x01\0\0\0")
    );
}

#[test]
fn generated_byte_access_rejects_code_changes_and_wrong_address_space() {
    for mutation in 0..3 {
        let mut memory = memory_with_code(0x1000, &[0x90]);
        let compiled =
            compile_region(&memory, &[block(0x1000, 1)], CompileLimits::default()).unwrap();
        assert!(compiled.wasm_bytes(&memory).is_ok());
        match mutation {
            0 => memory.write(GuestAddress(0x1000), &[0x90]).unwrap(),
            1 => memory.protect(range(0x1000, 1), Permissions::ALL).unwrap(),
            2 => {
                memory.unmap(range(0x1000, 1)).unwrap();
                assert!(matches!(
                    compiled.wasm_bytes(&memory),
                    Err(ArtifactError::CodeInvalidated)
                ));
                memory
                    .map_zeroed(range(0x1000, 1), Permissions::ALL)
                    .unwrap();
                memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(matches!(
            compiled.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        ));
    }
    let memory = memory_with_code(0x1000, &[0x90]);
    let other = memory_with_code(0x1000, &[0x90]);
    let compiled = compile_region(&memory, &[block(0x1000, 1)], CompileLimits::default()).unwrap();
    assert!(matches!(
        compiled.wasm_bytes(&other),
        Err(ArtifactError::CodeInvalidated)
    ));
}

#[test]
fn unrelated_guest_page_changes_preserve_generated_byte_access() {
    let mut memory = memory_with_code(0x1000, &[0x90]);
    let compiled = compile_region(&memory, &[block(0x1000, 1)], CompileLimits::default()).unwrap();
    let original = compiled.wasm_bytes(&memory).unwrap().to_vec();
    memory
        .map_zeroed(range(0x8000, 1), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(0x8000), &[0x91]).unwrap();
    memory.protect(range(0x8000, 1), Permissions::READ).unwrap();
    memory.unmap(range(0x8000, 1)).unwrap();
    assert_eq!(compiled.wasm_bytes(&memory).unwrap(), original);
}

#[test]
fn warmed_explicit_and_cold_artifacts_keep_memory_identity_and_code_versions() {
    for cold in [false, true] {
        let mut memory = memory_with_code(0x1000, &[0x90, 0xeb, 0xfe]);
        let mut other = memory_with_code(0x1000, &[0x90, 0xeb, 0xfe]);
        for space in [&mut memory, &mut other] {
            space
                .map_zeroed(range(0x8000, 1), Permissions::ALL)
                .unwrap();
            space.write(GuestAddress(0x8000), &[0]).unwrap();
        }
        let compiled = if cold {
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
        } else {
            compile_region(&memory, &[block(0x1000, 3)], CompileLimits::default())
        }
        .unwrap();
        let original = compiled.wasm_bytes(&memory).unwrap().to_vec();
        assert!(matches!(
            compiled.wasm_bytes(&other),
            Err(ArtifactError::CodeInvalidated)
        ));
        assert_eq!(compiled.wasm_bytes(&memory).unwrap(), original);

        memory.write(GuestAddress(0x8000), &[1]).unwrap();
        assert_eq!(compiled.wasm_bytes(&memory).unwrap(), original);
        assert_eq!(compiled.wasm_bytes(&memory).unwrap(), original);

        memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                compiled.wasm_bytes(&memory),
                Err(ArtifactError::CodeInvalidated)
            ));
        }
    }
}

#[test]
fn module_size_limit_rejects_artifact_larger_than_the_requested_budget() {
    let memory = memory_with_code(0x1000, &[0x90]);
    assert!(matches!(
        compile_region(
            &memory,
            &[block(0x1000, 1)],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            }
        ),
        Err(CompileError::WasmLimit)
    ));
}

#[test]
fn generated_modules_execute_in_node_against_independent_oracles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-baseline-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let baseline = [
        0xb8, 0, 0, 0, 0, 0x83, 0xf9, 0, 0x74, 7, 0x01, 0xc8, 0x83, 0xe9, 1, 0xeb, 0xf4,
    ];
    let memory = memory_with_code(0x1000, &baseline);
    let compiled = compile_region(
        &memory,
        &[block(0x1000, 5), block(0x1005, 5), block(0x100a, 7)],
        CompileLimits::default(),
    )
    .unwrap();
    fs::write(
        output.join("baseline.wasm"),
        compiled.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();

    for (name, pc, bytes) in [
        ("add", 0x1000, &[0x01, 0xd8][..]),
        ("sub", 0x1000, &[0x29, 0xd8][..]),
        ("cmp", 0x1000, &[0x39, 0xd8][..]),
        ("add_imm", 0x1000, &[0x83, 0xc0, 0xff][..]),
        ("sub_imm", 0x1000, &[0x83, 0xe8, 0x7f][..]),
        ("cmp_imm", 0x1000, &[0x3d, 0x78, 0x56, 0x34, 0x12][..]),
        ("alias_add", 0x1000, &[0x03, 0xf9][..]),
        ("alias_sub", 0x1000, &[0x2b, 0xd1][..]),
        ("alias_cmp", 0x1000, &[0x3b, 0xca][..]),
        ("add_same", 0x1000, &[0x01, 0xc0][..]),
        ("sub_same", 0x1000, &[0x29, 0xc0][..]),
        ("cmp_same", 0x1000, &[0x39, 0xc0][..]),
        ("mov_reg", 0x1000, &[0x89, 0xdf][..]),
        ("mov_imm", 0x1000, &[0xbe, 0x78, 0x56, 0x34, 0x12][..]),
        ("nop", 0x1000, &[0x90][..]),
        ("jump", 0x1000, &[0xeb, 5][..]),
        ("jump_negative", 0x1000, &[0xeb, 0xfe][..]),
        ("jump_wrap", 0xffff_ff00, &[0xe9, 0, 2, 0, 0][..]),
        ("nop_final", u32::MAX, &[0x90][..]),
    ] {
        let memory = memory_with_code(pc, bytes);
        let compiled = compile_region(
            &memory,
            &[block(pc, bytes.len() as u32)],
            CompileLimits::default(),
        )
        .unwrap();
        fs::write(
            output.join(format!("{name}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let mut dense = vec![0x90; 63];
    dense.extend_from_slice(&[0xeb, 0xbf]);
    let memory = memory_with_code(0x1000, &dense);
    let compiled = compile_region(&memory, &[block(0x1000, 65)], CompileLimits::default()).unwrap();
    assert_eq!(compiled.metadata().instructions, 64);
    fs::write(
        output.join("dense_resume.wasm"),
        compiled.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();
    for (condition, opcode) in [
        0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e,
        0x7f,
    ]
    .into_iter()
    .enumerate()
    {
        let memory = memory_with_code(0x1000, &[opcode, 2]);
        let compiled =
            compile_region(&memory, &[block(0x1000, 2)], CompileLimits::default()).unwrap();
        fs::write(
            output.join(format!("jcc_{condition}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/src/cpu/dbt/wasm/tests/run.mjs"))
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "Node harness failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
