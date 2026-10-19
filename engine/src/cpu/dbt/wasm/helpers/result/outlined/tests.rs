use super::*;
use crate::{
    abi::arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::dbt::wasm::helpers::result::{validate, validate_x87},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, ConstExpr, Encode, EntityType, ExportKind, ExportSection, FunctionSection,
    GlobalSection, GlobalType, ImportSection, MemoryType, Module, TypeSection,
};

mod read32;
mod write32;

fn wrapper(outlined: bool, length: u32, access: u32, allow_smc: bool) -> Function {
    let mut function = Function::new([(ADDRESS + 1 - INPUTS.len() as u32, ValType::I32)]);
    let mut code = function.instructions();
    for (parameter, local) in INPUTS.into_iter().enumerate().skip(1) {
        code.local_get(parameter as u32).local_set(local);
    }
    // parameter 1 has already been copied to HELPER_STATUS and now witnesses the exit branch.
    code.i32_const(0).local_set(1).block(BlockType::Empty);
    let emit = if length == 0 { validate } else { validate_x87 };
    emit(
        &mut code,
        length,
        access,
        allow_smc,
        0,
        ResultContext {
            paging_configured: true,
            paging_access_mask: None,
            zero_span_validator: outlined.then_some(2),
            x87_read32_validator: outlined.then_some(2),
            x87_write32_validator: outlined.then_some(2),
        },
    );
    code.i32_const(1).local_set(1).end();
    for local in OUTPUTS {
        code.local_get(local);
    }
    code.local_get(1).end();
    function
}

fn differential_module(read32: bool) -> Vec<u8> {
    differential_span_module(if read32 { 4 } else { 0 }, u32::from(read32), false)
}

fn differential_span_module(length: u32, access: u32, allow_smc: bool) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 12], [ValType::I32; 9]);
    types.ty().function([ValType::I32; 12], [ValType::I32; 8]);
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
    functions.function(0).function(0).function(1).function(1);
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
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    module.section(&globals);
    let mut exports = ExportSection::new();
    exports.export("inline", ExportKind::Func, 0);
    exports.export("outlined", ExportKind::Func, 1);
    exports.export("slowCalls", ExportKind::Global, 3);
    module.section(&exports);
    let mut bodies = CodeSection::new();
    bodies.function(&wrapper(false, length, access, allow_smc));
    bodies.function(&wrapper(true, length, access, allow_smc));
    let mut counted = Function::new([]);
    let mut code = counted.instructions();
    code.global_get(3).i32_const(1).i32_add().global_set(3);
    for parameter in 0..12 {
        code.local_get(parameter);
    }
    code.call(3).end();
    bodies.function(&counted);
    bodies.function(&build_span_function(length, access, allow_smc));
    module.section(&bodies);
    module.finish()
}

fn node(script: &str, input: &str) -> String {
    let mut child = Command::new("node")
        .args(["--input-type=module", "-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available for generated helper proof");
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
    String::from_utf8(output.stdout).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn generated_zero_span_matches_inline_failure_priority_scratch_and_live_context() {
    let input = format!(
        "{{\"module\":\"{}\",\"segments\":{},\"bases\":{}}}",
        hex(&differential_module(false)),
        SEGMENTS_OFFSET,
        SEGMENT_BASES_RECORD_OFFSET,
    );
    assert_eq!(
        node(include_str!("fixtures/runtime/run.mjs"), &input).trim(),
        "zero-span inline/outlined differential controls passed",
    );
}

#[test]
fn zero_span_outline_requires_the_exact_contract_and_an_available_local_function() {
    for (length, access, smc, paging, intent, has_helper) in [
        (0, 0, false, false, None, true),
        (0, 0, false, true, Some(1), true),
        (0, 0, false, true, Some(2), true),
        (0, 0, true, true, None, true),
        (4, 1, false, true, None, true),
        (0, 0, false, true, None, false),
    ] {
        let context = ResultContext {
            paging_configured: paging,
            paging_access_mask: intent,
            zero_span_validator: has_helper.then_some(42),
            x87_read32_validator: has_helper.then_some(43),
            x87_write32_validator: has_helper.then_some(44),
        };
        let mut actual = Function::new([(36, ValType::I32)]);
        let mut retained = Function::new([(36, ValType::I32)]);
        validate(&mut actual.instructions(), length, access, smc, 0, context);
        validate_span(
            &mut retained.instructions(),
            length,
            access,
            smc,
            Validation::Ordinary { mask: None },
            0,
            context,
        );
        let mut before = Vec::new();
        let mut after = Vec::new();
        retained.encode(&mut before);
        actual.encode(&mut after);
        assert_eq!(after, before, "nonmatching contract stays byte-identical");
    }
}

#[test]
fn ordinary_helpers_count_real_x87_sites_and_compose_with_scalar_and_segment_helpers() {
    use crate::{
        cpu::{
            dbt::{
                CompileLimits,
                cold::prepare_runtime_entry,
                wasm::{EmbeddedBinding, emitter::emit, requirements::ModuleRequirements},
            },
            x86::{
                SegmentContext,
                decode::{CodeFetchContext, DecodeAuthority},
            },
        },
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    let cases: [(&[u8], bool, bool, usize, usize); 10] = [
        (&[0x90, 0xff, 0xe2], false, false, 0, 0),
        (&[0xd9, 0x00, 0xff, 0xe2], false, true, 2, 2),
        (&[0xdb, 0x00, 0xff, 0xe2], false, true, 2, 2),
        (&[0xd8, 0x00, 0xff, 0xe2], false, true, 2, 2),
        (&[0xdd, 0x00, 0xff, 0xe2], false, true, 0, 0),
        (&[0xd9, 0x10, 0xff, 0xe2], false, true, 0, 0),
        (&[0xd9, 0xe8, 0xff, 0xe2], true, false, 0, 0),
        (&[0xd9, 0xe8, 0xd9, 0x00, 0xff, 0xe2], true, true, 2, 3),
        (&[0xdf, 0xe9, 0xff, 0xe2], true, false, 0, 0),
        (
            &[0xd9, 0x00, 0xd9, 0xe8, 0x8b, 0x03, 0x8b, 0x0b, 0xff, 0xe2],
            true,
            true,
            2,
            2,
        ),
    ];
    let mut modules = Vec::new();
    for (code, outlined, segments, scalar_sites, resident_sites) in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), code).unwrap();
        let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
        let prepared = prepare_runtime_entry(
            &memory,
            GuestAddress(0x1000),
            CompileLimits::default(),
            Some(cs),
            DecodeAuthority::new(None),
        )
        .unwrap();
        for binding in [
            EmbeddedBinding::Replacement {
                key: 7,
                generation: 1,
            },
            EmbeddedBinding::Resident { key: 7, id: 1 },
        ] {
            let plan = ModuleRequirements::new(&prepared.blocks, Some(binding));
            assert_eq!(plan.has_zero_span_validator, outlined);
            assert_eq!(plan.has_segment_check, segments);
            assert_eq!(plan.has_x87_read32_validator, scalar_sites >= 2);
            take_read32_sites();
            let bytes = emit(&prepared.blocks, Some(binding), Some(cs.decode_key()));
            let emitted_sites = match binding {
                EmbeddedBinding::Replacement { .. } => scalar_sites,
                EmbeddedBinding::Resident { .. } => resident_sites,
            };
            assert_eq!(
                take_read32_sites(),
                emitted_sites,
                "waiting prechecks and pair branches count as real emitted sites"
            );
            modules.push(format!("\"{}\"", hex(&bytes)));
        }
    }
    let script = "import assert from 'node:assert/strict'; let input=''; for await(const c of process.stdin) input+=c; const modules=JSON.parse(input); assert.equal(modules.length,20); for(const [i,hex] of modules.entries()) assert.ok(WebAssembly.validate(Buffer.from(hex,'hex')),`module ${i}`); console.log('20 generated modules validated');";
    assert_eq!(
        node(script, &format!("[{}]", modules.join(","))).trim(),
        "20 generated modules validated"
    );
}
