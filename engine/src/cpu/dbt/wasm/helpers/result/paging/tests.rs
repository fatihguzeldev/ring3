use super::super::{Validation as LegacyValidation, *};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, ConstExpr, Encode, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, MemArg, MemoryType, Module,
    TypeSection, ValType,
};

#[test]
fn standalone_helper_validation_preserves_legacy_packet_contract() {
    for (length, access, smc, validation) in [
        (4, 1, false, LegacyValidation::Ordinary { mask: None }),
        (8, 3, true, LegacyValidation::Ordinary { mask: None }),
        (
            16,
            2,
            true,
            LegacyValidation::Ordinary { mask: Some(RESUME) },
        ),
        (
            32,
            1,
            false,
            LegacyValidation::Avx(AvxValidation {
                selected_mask: Some(RESUME),
                old_mxcsr: None,
                general_protection: true,
                stack_segment: true,
            }),
        ),
        (832, 3, true, LegacyValidation::XState),
        (2, 2, true, LegacyValidation::Gate),
    ] {
        let mut retained = Function::new([(36, ValType::I32)]);
        let mut current = Function::new([(36, ValType::I32)]);
        validate_legacy_span(
            &mut retained.instructions(),
            length,
            access,
            smc,
            validation,
            0,
        );
        validate_span(
            &mut current.instructions(),
            length,
            access,
            smc,
            validation,
            0,
            ResultContext::legacy().write_intent(),
        );
        let mut before = Vec::new();
        let mut after = Vec::new();
        retained.encode(&mut before);
        current.encode(&mut after);
        assert_eq!(after, before);
    }
}

fn module(length: u32, access: u32, masked: bool, transport: u32, address: u32) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], []);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: Some(1),
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for value in [0, 0, 1] {
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
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([(36, ValType::I32)]);
    let mut code = function.instructions();
    for (local, value) in [
        (ADDRESS, address),
        (HELPER_STATUS, transport),
        (EIP, 0x1000),
        (FLAGS, 2),
        (ORIGINAL_BUDGET, 0x10002),
        (SNAPSHOT_SHADOW, 1),
        (RESUME, 1),
    ] {
        code.i32_const(value as i32).local_set(local);
    }
    code.block(BlockType::Empty);
    let context = ResultContext {
        paging_configured: true,
        paging_access_mask: None,
    };
    if masked {
        validate_masked(&mut code, length, RESUME, 0, context);
    } else {
        validate(&mut code, length, access, true, 0, context);
    }
    code.end();
    for (index, local) in [
        REASON,
        DETAIL,
        FAULT_ADDRESS,
        FAULT_ACCESS,
        FAULT_LENGTH,
        FLAGS,
        DEBUG_SHADOW,
        EIP,
        RETIRED,
    ]
    .into_iter()
    .enumerate()
    {
        code.i32_const(32768).local_get(local).i32_store(MemArg {
            offset: (index * 4) as u64,
            align: 2,
            memory_index: 0,
        });
    }
    code.end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}

#[test]
fn typed_generated_packets_validate_intent_wrap_mask_and_malformed_controls() {
    // packet fields are independent literals: status, value, detail, linear, access, geometry.
    type Case = (u16, [u32; 6], u32, u32, bool, u32, u32, [u32; 7]);
    let cases: [Case; 21] = [
        (
            15,
            [9, 0, 4, 0, 1, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [23, 4, 0, 1, 4, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0, 2, 4],
            4,
            2,
            false,
            23,
            0xffff_fffe,
            [23, 6, 0, 2, 4, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0, 1, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 0, 0, 1, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 8, 0, 1, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [8, 0, 1, 0, 0x101, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [8, 0, 4, 0, 0x101, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [8, 0, 5, 0, 0x101, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [8, 0, 1, 0, 1, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 4, 0, 1, 3],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0x1001, 2, 4],
            4,
            2,
            true,
            23,
            0x1000,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0x1000, 2, 4],
            4,
            2,
            true,
            23,
            0x1000,
            [23, 6, 0x1000, 2, 4, 2, 0],
        ),
        (
            15,
            [0, 0, 0, 0, 0, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 4, 0, 1, 4],
            4,
            1,
            false,
            7,
            0xffff_fffe,
            [7, 3, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 4, 0, 1, 4],
            4,
            1,
            false,
            11,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0x11cf, 2, 512],
            512,
            2,
            false,
            23,
            0x1000,
            [23, 6, 0x11cf, 2, 512, 2, 0],
        ),
        (
            15,
            [9, 0, 6, 0x11d0, 2, 512],
            512,
            2,
            false,
            23,
            0x1000,
            [23, 6, 0x11d0, 2, 512, 2, 0],
        ),
        (
            16,
            [9, 0, 4, 0, 1, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [9, 0, 4, 0, 1, 4],
            4,
            1,
            false,
            0,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            7,
            [0, 0, 0, 0, 0, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
        (
            15,
            [8, 0, 1, 0, 0x101, 4],
            4,
            1,
            false,
            23,
            0xffff_fffe,
            [7, 2, 0, 0, 0, 2, 0],
        ),
    ];
    let mut rows = Vec::new();
    for (version, fields, length, access, masked, transport, address, expected) in cases {
        let bytes = module(length, access, masked, transport, address);
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        rows.push(format!("{{\"module\":\"{hex}\",\"version\":{version},\"fields\":{fields:?},\"expected\":{expected:?}}}"));
    }
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
        .expect("node available for generated validator proof");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", rows.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "21 generated application memory packet controls passed"
    );
}

#[test]
fn bound_application_stack_xstate_numeric_far_and_int_modules_keep_valid_branch_structure() {
    use crate::{
        cpu::{
            dbt::{
                CompileLimits,
                cold::prepare_runtime_entry,
                wasm::{EmbeddedBinding, emitter::emit},
            },
            x86::{
                SegmentContext,
                decode::{CodeFetchContext, DecodeAuthority},
            },
        },
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    let cases: [&[u8]; 11] = [
        &[0x60, 0xeb, 0xfe],
        &[0x61, 0xeb, 0xfe],
        &[0xc8, 0x12, 0, 3, 0xeb, 0xfe],
        &[0x0f, 0xae, 0x20, 0xeb, 0xfe],
        &[0x0f, 0xae, 0x28, 0xeb, 0xfe],
        &[0xd9, 0x00, 0xeb, 0xfe],
        &[0xf3, 0x0f, 0x6f, 0, 0xeb, 0xfe],
        &[0x9a, 0, 0x10, 0, 0, 0x1b, 0],
        &[0xcb],
        &[0xcf],
        &[0xcd, 0x80],
    ];
    let mut modules = Vec::new();
    for bytes in cases {
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
            DecodeAuthority::new(None),
        )
        .unwrap();
        let bytes = emit(
            &prepared.blocks,
            Some(EmbeddedBinding::Replacement {
                key: 7,
                generation: 1,
            }),
            Some(cs.decode_key()),
        );
        modules.push(format!(
            "\"{}\"",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
    }
    let script = "import assert from 'node:assert/strict'; let input=''; for await (const chunk of process.stdin) input+=chunk; const rows=JSON.parse(input); assert.equal(rows.length,11); for(const [i,hex] of rows.entries()) assert.ok(WebAssembly.validate(Buffer.from(hex,'hex')),`generated owner ${i}`); console.log('11 generated paging owner modules validated');";
    let mut child = Command::new("node")
        .args(["--input-type=module", "-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available for generated structure proof");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", modules.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "11 generated paging owner modules validated"
    );
}
