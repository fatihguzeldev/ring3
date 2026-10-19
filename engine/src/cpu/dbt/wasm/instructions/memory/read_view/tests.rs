use super::*;
use crate::{
    abi::arena::{self, ARENA_SIZE, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::{
        dbt::{
            CompileLimits,
            cold::prepare_runtime_entry,
            wasm::{
                EmbeddedBinding, detection::DetectionLocals, emitter::emit_with_tier, locals::*,
                requirements::ModuleRequirements,
            },
        },
        x86::{
            SegmentContext,
            decode::{CodeFetchContext, DecodeAuthority},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    GlobalSection, GlobalType, ImportSection, MemoryType, Module, TypeSection, ValType,
};

fn wrapper() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 2], [ValType::I32; 10]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I64], [ValType::I32]);
    types.ty().function([], []);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 128,
            maximum: Some(128),
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    for (name, ty) in [("cpu_read32", 1), ("cpu_read32_view", 2), ("barrier", 3)] {
        imports.import("ring3", name, EntityType::Function(ty));
    }
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for value in [1, 0, 1] {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(value),
        );
    }
    module.section(&globals);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 3);
    module.section(&exports);
    let mut function = Function::new([
        (25, ValType::I32),
        (1, ValType::I64),
        (13, ValType::I32),
        (1, ValType::I64),
        (2, ValType::I32),
    ]);
    let mut code = function.instructions();
    code.local_get(0)
        .local_set(ADDRESS)
        .local_get(1)
        .local_set(42);
    for (local, value) in [
        (STATE_PTR, 0),
        (EIP, 0x1000),
        (REASON, 0),
        (DETAIL, 0x91),
        (FAULT_ADDRESS, 0xa2),
        (FAULT_ACCESS, 0xb3),
        (FAULT_LENGTH, 0xc4),
        (LHS, 0xd5),
        (RHS, 0xe6),
        (RESULT, 0xf7),
    ] {
        code.i32_const(value).local_set(local);
    }
    code.i32_const(520).i64_load(arg(0, 3)).local_set(27);
    let imports = Imports {
        paging_configured: true,
        read: Some(0),
        read32_view: Some(1),
        read32_view_locals: Some(ReadViewLocals::new(34)),
        detection: Some(DetectionLocals::new(27, false)),
        ..Imports::default()
    };
    initialize(&mut code, imports);
    code.block(BlockType::Empty)
        .local_get(42)
        .i32_eqz()
        .br_if(0)
        .loop_(BlockType::Empty);
    super::super::scalar::load_result_at_address(&mut code, imports, 1);
    code.local_get(43)
        .i32_const(1)
        .i32_add()
        .local_tee(43)
        .local_get(42)
        .i32_ge_u()
        .br_if(1);
    code.call(2).br(0).end().end();
    for local in [
        HELPER_STATUS,
        REASON,
        DETAIL,
        FAULT_ADDRESS,
        FAULT_ACCESS,
        FAULT_LENGTH,
        LHS,
        RHS,
        RESULT,
        43,
    ] {
        code.local_get(local);
    }
    code.end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn generated_read32_view_checks_live_metadata_and_preserves_fallback_protocol() {
    let cases: [(&str, &[u8], bool); 8] = [
        ("pure", &[0x90, 0xff, 0xe2], false),
        ("read", &[0x8b, 0x03, 0xff, 0xe2], true),
        ("modify", &[0x83, 0x03, 1, 0xff, 0xe2], false),
        ("narrow", &[0x8a, 0x03, 0xff, 0xe2], false),
        ("x87", &[0xd9, 0x03, 0xff, 0xe2], false),
        ("repeat", &[0xf3, 0xa5, 0xff, 0xe2], false),
        ("mixed_string", &[0x8b, 0x03, 0xa5, 0xff, 0xe2], false),
        ("store_read", &[0x89, 0x03, 0x8b, 0x03, 0xff, 0xe2], true),
    ];
    let mut modules = Vec::new();
    for (name, bytes, eligible) in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
        let prepared = prepare_runtime_entry(
            &memory,
            GuestAddress(0x1000),
            CompileLimits::default(),
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
        for resident in [false, true] {
            let binding = if resident {
                EmbeddedBinding::Resident { key: 7, id: 1 }
            } else {
                EmbeddedBinding::Replacement {
                    key: 7,
                    generation: 1,
                }
            };
            let expected = resident && eligible;
            assert_eq!(
                ModuleRequirements::new(&prepared.blocks, Some(binding)).has_read32_view,
                expected,
                "{name}/{resident}"
            );
            for tier in [
                crate::cpu::dbt::CompilationTier32::Baseline,
                crate::cpu::dbt::CompilationTier32::Optimized,
            ] {
                modules.push(format!(
                    "{{\"name\":\"{name}\",\"view\":{expected},\"bytes\":\"{}\"}}",
                    hex(&emit_with_tier(
                        &prepared.blocks,
                        Some(binding),
                        Some(cs.decode_key()),
                        tier
                    ))
                ));
            }
        }
    }
    let mut arena = vec![0; ARENA_SIZE];
    arena::initialize(&mut arena).unwrap();
    let input = format!(
        "{{\"wrapper\":\"{}\",\"arena\":\"{}\",\"segments\":{SEGMENTS_OFFSET},\"bases\":{SEGMENT_BASES_RECORD_OFFSET},\"modules\":[{}]}}",
        hex(&wrapper()),
        hex(&arena),
        modules.join(",")
    );
    let mut child = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            include_str!("fixtures/runtime/run.mjs"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8(output.stdout).unwrap());
}
