use super::*;
use crate::memory::{PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(4).unwrap();
    for (address, permissions) in [
        (0x1000, Permissions::ALL),
        (0x2000, Permissions::ALL),
        (0x3000, Permissions::READ_WRITE),
        (0x4000, Permissions::READ_WRITE),
    ] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                permissions,
            )
            .unwrap();
    }
    memory
}

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn image() -> [u8; 4096] {
    let mut image = [0; 4096];
    image[..10].copy_from_slice(&[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b]);
    image[0x40..0x4a].copy_from_slice(&[0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 0x0c, 1, 0xc3]);
    image
}

#[test]
fn bounded_leaf_discovery_closes_the_call_cycle_with_the_existing_limits() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    let normal = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    let closed = compile_formation(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[],
        &[
            GuestAddress(0x1040),
            GuestAddress(0x1000),
            GuestAddress(0x1040),
        ],
    )
    .unwrap();
    let limited = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 4,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(limited.instruction_count(), 4);
    assert_eq!(
        limited.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1005),
            GuestAddress(0x1008),
        ]
    );
    assert_eq!(closed.instruction_count(), 8);
    assert_eq!(normal.instruction_count(), 8);
    assert_eq!(normal.entrypoints(), closed.entrypoints());
    assert_eq!(
        closed.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1005),
            GuestAddress(0x1008),
            GuestAddress(0x1040),
        ]
    );
    assert!(normal.native_instructions().is_empty());
    assert!(closed.native_instructions().is_empty());
    assert!(normal.is_current(&memory));
    assert!(closed.is_current(&memory));
}

#[test]
fn explicit_roots_respect_the_block_and_instruction_limits() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    assert!(matches!(
        compile_formation(
            &memory,
            GuestAddress(0x1000),
            CompileOptions {
                blocks: 1,
                ..options()
            },
            &[],
            &[],
            &[GuestAddress(0x1040)],
        ),
        Err(CompileError::InvalidLimits)
    ));
    let limited = compile_formation(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 4,
            ..options()
        },
        &[],
        &[],
        &[GuestAddress(0x1040)],
    )
    .unwrap();
    assert_eq!(limited.instruction_count(), 4);
    assert!(limited.entrypoints().contains(&GuestAddress(0x1000)));
    assert!(limited.wasm().len() <= options().wasm_bytes);
    assert!(matches!(
        compile_formation(
            &memory,
            GuestAddress(0x1000),
            options(),
            &[],
            &[GuestAddress(0x1040)],
            &[GuestAddress(0x1040)],
        ),
        Err(CompileError::InvalidLimits)
    ));
}

#[test]
fn an_explicit_callee_owns_its_source_and_reached_gate_validation() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0, 0x0f, 0x0b])
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0x40, 0xc3]).unwrap();
    let normal = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 2,
            ..options()
        },
    )
    .unwrap();
    let automatic = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    let closed = compile_formation(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[],
        &[GuestAddress(0x2000)],
    )
    .unwrap();
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(normal.is_current(&memory));
    assert!(!automatic.is_current(&memory));
    assert!(!closed.is_current(&memory));
    memory.write(GuestAddress(0x2000), &[0x0f, 0x0b]).unwrap();
    assert!(matches!(
        compile_formation(
            &memory,
            GuestAddress(0x1000),
            options(),
            &[(GuestAddress(0x2000), 0)],
            &[],
            &[GuestAddress(0x2000)],
        ),
        Err(CompileError::InvalidLimits)
    ));
}

#[test]
fn a_proven_leaf_crosses_only_its_own_cached_frontiers() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    let code = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[
            GuestAddress(0x1005),
            GuestAddress(0x1040),
            GuestAddress(0x1043),
        ],
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 5);
    assert_eq!(
        code.entrypoints(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    assert_eq!(
        code.instruction_pcs().collect::<Vec<_>>(),
        [0x1000, 0x1040, 0x1043, 0x1045, 0x1049]
    );
    memory.write(GuestAddress(0x3000), &[1]).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x1041), &[0xc1]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn explicit_formation_rebuilds_owned_leaves_without_crossing_another_owners_frontier() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    memory
        .write(GuestAddress(0x1040), &[0xe8, 0x3b, 0, 0, 0, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x1080), &[0x40, 0xc3]).unwrap();
    let owner = compile(&memory, GuestAddress(0x1040), options()).unwrap();
    assert!(owner.entrypoints().contains(&GuestAddress(0x1080)));
    let roots = [GuestAddress(0x1000), GuestAddress(0x1040)];
    let rebuilt = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1040),
        options(),
        &[],
        &[],
        &roots,
    )
    .unwrap()
    .unwrap();
    assert_eq!(rebuilt.instruction_count(), 8);
    for pc in owner.entrypoints() {
        assert!(rebuilt.entrypoints().contains(pc));
    }
    for frontier in [0x1080, 0x1081] {
        let limited = CompiledCode::compile_transition(
            &memory,
            GuestAddress(0x1040),
            options(),
            &[],
            &[GuestAddress(frontier)],
            &roots,
        )
        .unwrap()
        .unwrap();
        assert_eq!(limited.instruction_count(), 6);
        assert!(!limited.entrypoints().contains(&GuestAddress(0x1080)));
        assert!(
            !limited
                .instruction_pcs()
                .any(|pc| matches!(pc, 0x1080 | 0x1081))
        );
        assert!(limited.entrypoints().contains(&GuestAddress(0x1000)));
        assert!(limited.entrypoints().contains(&GuestAddress(0x1040)));
    }
}

#[test]
fn leaf_discovery_declines_branches_nested_calls_gates_and_missing_returns() {
    for callee in [
        vec![0xeb, 0, 0xc3],
        vec![0xe8, 0, 0, 0, 0, 0xc3],
        vec![0xcd, 0x80, 0xc3],
        vec![0x0f, 0x0b],
        vec![0x90; 32],
    ] {
        let mut memory = memory();
        memory.write(GuestAddress(0x1000), &image()).unwrap();
        memory.write(GuestAddress(0x1040), &callee).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
        assert_eq!(code.instruction_count(), 4);
        assert!(!code.entrypoints().contains(&GuestAddress(0x1040)));
    }
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    memory.write(GuestAddress(0x1040), &[0x0f, 0x0b]).unwrap();
    let code = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[(GuestAddress(0x1040), 77)],
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 4);
    assert!(!code.entrypoints().contains(&GuestAddress(0x1040)));
}

#[test]
fn leaf_discovery_respects_the_leaf_instruction_and_block_caps() {
    for (nops, included) in [(31, true), (32, false)] {
        let mut memory = memory();
        memory.write(GuestAddress(0x1000), &image()).unwrap();
        let mut callee = vec![0x90; nops];
        callee.push(0xc3);
        memory.write(GuestAddress(0x1040), &callee).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
        assert_eq!(code.entrypoints().contains(&GuestAddress(0x1040)), included);
        assert_eq!(code.instruction_count(), if included { 36 } else { 4 });
    }
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &image()).unwrap();
    let code = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            blocks: 1,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
    assert_eq!(code.instruction_count(), 1);
}

#[test]
fn speculative_leaf_fetch_failure_does_not_fail_the_caller() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x3f, 0, 0, 0x0f, 0x0b])
        .unwrap();
    let code = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    assert_eq!(
        code.entrypoints(),
        &[GuestAddress(0x1000), GuestAddress(0x1005)]
    );
    assert_eq!(code.instruction_count(), 2);
    assert!(code.is_current(&memory));
}

fn fixture_resolver() -> Vec<u8> {
    use wasm_encoder::{
        BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
        ImportSection, MemArg, MemoryType, Module, TypeSection, ValType,
    };

    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "ring3",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("next", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([]);
    for (pc, slot, owner_offset) in [
        (0x1040, 2, 8),
        (0x1000, 1, 0),
        (0x1005, 1, 0),
        (0x1008, 1, 0),
    ] {
        let mem = |offset, align| MemArg {
            offset,
            align,
            memory_index: 0,
        };
        function
            .instructions()
            .local_get(0)
            .i32_load(mem(32, 2))
            .i32_const(pc)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(0)
            .i32_const(0x510400 + owner_offset)
            .i64_load(mem(0, 3))
            .i64_store(mem(120, 3))
            .i32_const(slot)
            .return_()
            .end();
    }
    function.instructions().i32_const(0).end();
    let mut code = CodeSection::new();
    code.function(&function);
    module.section(&code);
    module.finish()
}

#[test]
fn generated_call_regions_preserve_precise_prefixes_and_memory() {
    use std::{
        fmt::Write,
        fs,
        path::{Path, PathBuf},
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    let retained = std::env::var_os("RING3_CORE_CALL_FIXTURE").map(PathBuf::from);
    let directory = retained.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "ring3-core-call-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    });
    fs::create_dir(&directory).unwrap();
    let mut manifest = String::new();
    for variant in ["plain", "gate"] {
        let mut image = image();
        let gates = if variant == "gate" {
            image[0x45..0x47].copy_from_slice(&[0x0f, 0x0b]);
            vec![(GuestAddress(0x1045), 77)]
        } else {
            Vec::new()
        };
        let mut memory = memory();
        memory.write(GuestAddress(0x1000), &image).unwrap();
        fs::write(directory.join(format!("{variant}.bin")), image).unwrap();
        let caller = compile_with_gates(
            &memory,
            GuestAddress(0x1000),
            CompileOptions {
                instructions: 4,
                ..options()
            },
            &gates,
        )
        .unwrap();
        let callee = compile_with_gates(&memory, GuestAddress(0x1040), options(), &gates).unwrap();
        let closed = if variant == "plain" {
            compile_with_gates(&memory, GuestAddress(0x1000), options(), &gates).unwrap()
        } else {
            compile_formation(
                &memory,
                GuestAddress(0x1000),
                options(),
                &gates,
                &[],
                &[GuestAddress(0x1040)],
            )
            .unwrap()
        };
        for (suffix, code) in [("caller", caller), ("callee", callee), ("closed", closed)] {
            let name = format!("{variant}-{suffix}");
            assert!(code.is_current(&memory));
            fs::write(directory.join(format!("{name}.wasm")), code.wasm()).unwrap();
            writeln!(
                &mut manifest,
                "{name}\t{}\t{}",
                code.identity(),
                code.instruction_count()
            )
            .unwrap();
        }
    }
    fs::write(directory.join("regions.tsv"), manifest).unwrap();
    fs::write(directory.join("resolver.wasm"), fixture_resolver()).unwrap();
    fs::write(
        directory.join("driver.wasm"),
        crate::cpu::core::emit_dispatch(),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cpu/core/fixtures/call-closure.mjs"))
        .arg(&directory)
        .output()
        .expect("run generated call fixture");
    assert!(
        result.status.success(),
        "call fixture retained at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    if retained.is_none() {
        fs::remove_dir_all(directory).unwrap();
    }
}
