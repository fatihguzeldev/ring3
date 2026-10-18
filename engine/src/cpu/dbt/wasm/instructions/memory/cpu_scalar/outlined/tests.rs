use super::*;
use crate::{
    abi::arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
    cpu::dbt::wasm::{
        helpers::protocol::exit_if_failed,
        imports::Imports,
        instructions::memory::cpu_scalar::validate_read32,
        locals::{EIP, ORIGINAL_BUDGET, RESULT},
    },
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, ConstExpr, Encode, EntityType, ExportKind, ExportSection, FunctionSection,
    GlobalSection, GlobalType, ImportSection, MemoryType, Module, TypeSection,
};

const CALLER_INPUTS: [u32; 12] = [
    STATE_PTR,
    HELPER_STATUS,
    EIP,
    ADDRESS,
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
    RESULT,
];
const CALLER_OUTPUTS: [u32; 8] = [
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
    RESULT,
];

fn context(index: Option<u32>) -> Imports {
    Imports {
        paging_configured: true,
        scalar_read32_validator: index,
        ..Imports::default()
    }
}

fn wrapper(outlined: bool, precheck_context: bool, count: usize, helper: u32) -> Function {
    let mut function = Function::new([(ADDRESS + 1 - CALLER_INPUTS.len() as u32, ValType::I32)]);
    let mut code = function.instructions();
    for (parameter, local) in CALLER_INPUTS.into_iter().enumerate().skip(1) {
        code.local_get(parameter as u32).local_set(local);
    }
    // the transport parameter is now copied; reuse its slot as a continuation witness.
    code.i32_const(0).local_set(1).block(BlockType::Empty);
    for _ in 0..count {
        if precheck_context {
            crate::cpu::dbt::wasm::code_fetch::check_context(&mut code, 0);
        }
        validate_read32(&mut code, context(outlined.then_some(helper)));
        exit_if_failed(&mut code, 0);
    }
    code.i32_const(1).local_set(1).end();
    for local in CALLER_OUTPUTS {
        code.local_get(local);
    }
    code.local_get(1).end();
    function
}

fn loop_function(outlined: bool) -> Function {
    let mut function = Function::new([(ORIGINAL_BUDGET, ValType::I32)]);
    let mut code = function.instructions();
    code.local_get(0).local_set(ORIGINAL_BUDGET);
    for (local, value) in [
        (STATE_PTR, 0),
        (HELPER_STATUS, 0),
        (EIP, 0x1000),
        (ADDRESS, -2),
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
    code.block(BlockType::Empty)
        .local_get(ORIGINAL_BUDGET)
        .i32_eqz()
        .br_if(0)
        .loop_(BlockType::Empty);
    validate_read32(&mut code, context(outlined.then_some(4)));
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .unreachable()
        .end();
    code.local_get(ORIGINAL_BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_tee(ORIGINAL_BUDGET)
        .br_if(0)
        .end()
        .end()
        .local_get(RESULT)
        .end();
    function
}

fn globals(counter: bool) -> GlobalSection {
    let mut globals = GlobalSection::new();
    for (index, value) in [1, 0, 1, 0].into_iter().enumerate() {
        if index == 3 && !counter {
            break;
        }
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: index == 3,
                shared: false,
            },
            &ConstExpr::i32_const(value),
        );
    }
    globals
}

fn memory_imports() -> ImportSection {
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
    imports
}

fn differential_module() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 12], [ValType::I32; 9]);
    types.ty().function([ValType::I32; 10], [ValType::I32; 7]);
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types).section(&memory_imports());
    let mut functions = FunctionSection::new();
    for ty in [0, 0, 0, 0, 1, 1, 2, 2] {
        functions.function(ty);
    }
    module.section(&functions).section(&globals(true));
    let mut exports = ExportSection::new();
    for (index, name) in [
        "inline",
        "outlined",
        "inline_context",
        "outlined_context",
        "inline_loop",
        "outlined_loop",
    ]
    .into_iter()
    .enumerate()
    {
        let function = if index < 4 { index } else { index + 2 };
        exports.export(name, ExportKind::Func, function as u32);
    }
    exports.export("slowCalls", ExportKind::Global, 3);
    module.section(&exports);
    let mut bodies = CodeSection::new();
    for precheck in [false, true] {
        bodies.function(&wrapper(false, precheck, 1, 4));
        bodies.function(&wrapper(true, precheck, 1, 4));
    }
    let mut counted = Function::new([]);
    let mut code = counted.instructions();
    code.global_get(3).i32_const(1).i32_add().global_set(3);
    for parameter in 0..10 {
        code.local_get(parameter);
    }
    code.call(5).end();
    bodies.function(&counted).function(&build_function());
    bodies
        .function(&loop_function(false))
        .function(&loop_function(true));
    module.section(&bodies);
    module.finish()
}

fn size_module(count: usize, outlined: bool) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 12], [ValType::I32; 9]);
    if outlined {
        types.ty().function([ValType::I32; 10], [ValType::I32; 7]);
    }
    module.section(&types).section(&memory_imports());
    let mut functions = FunctionSection::new();
    functions.function(0);
    if outlined {
        functions.function(1);
    }
    module.section(&functions).section(&globals(false));
    let mut bodies = CodeSection::new();
    bodies.function(&wrapper(outlined, true, count, 1));
    if outlined {
        bodies.function(&build_function());
    }
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
fn generated_read32_matches_full_validator_and_preserves_live_context_priority() {
    let input = format!(
        "{{\"module\":\"{}\",\"segments\":{},\"bases\":{}}}",
        hex(&differential_module()),
        SEGMENTS_OFFSET,
        SEGMENT_BASES_RECORD_OFFSET,
    );
    if let Some(path) = std::env::var_os("RING3_READ32_CONTROL_OUTPUT") {
        std::fs::write(path, &input).unwrap();
    }
    assert_eq!(
        node(include_str!("fixtures/runtime/run.mjs"), &input).trim(),
        "read32 inline/outlined controls passed"
    );
}

#[test]
fn unbound_and_unavailable_read32_helpers_retain_original_bytes() {
    for (paging, helper) in [(false, Some(42)), (true, None), (false, None)] {
        let imports = Imports {
            paging_configured: paging,
            scalar_read32_validator: helper,
            ..Imports::default()
        };
        let mut actual = Function::new([(36, ValType::I32)]);
        let mut retained = Function::new([(36, ValType::I32)]);
        validate_read32(&mut actual.instructions(), imports);
        validate(
            &mut retained.instructions(),
            4,
            CpuScalarOperation::Read,
            paging,
        );
        let (mut before, mut after) = (Vec::new(), Vec::new());
        retained.encode(&mut before);
        actual.encode(&mut after);
        assert_eq!(after, before);
    }
}

#[test]
fn shared_failure_body_shrinks_multiple_sites_and_exposes_single_site_overhead() {
    let mut sizes = Vec::new();
    for count in [1, 2, 8, 32] {
        let inline = size_module(count, false);
        let outlined = size_module(count, true);
        let input = format!("[\"{}\",\"{}\"]", hex(&inline), hex(&outlined));
        node(
            "import assert from 'node:assert/strict'; let s=''; for await(const c of process.stdin)s+=c; for(const x of JSON.parse(s))assert.ok(WebAssembly.validate(Buffer.from(x,'hex')));",
            &input,
        );
        sizes.push((count, inline.len(), outlined.len()));
    }
    assert!(
        sizes[0].2 > sizes[0].1,
        "single-site overhead remains visible: {sizes:?}"
    );
    assert!(
        sizes[1..].iter().all(|row| row.2 < row.1),
        "shared cold failure body: {sizes:?}"
    );
    println!("read32 authored site counts/inline bytes/outlined bytes: {sizes:?}");
}

#[test]
fn only_real_read32_sites_add_the_helper_and_mixed_helpers_validate() {
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
    let cases: [(&[u8], usize); 17] = [
        (&[0x90, 0xff, 0xe2], 0),
        (&[0x8a, 0x03, 0xff, 0xe2], 0),
        (&[0x66, 0x8b, 0x03, 0xff, 0xe2], 0),
        (&[0x89, 0x03, 0xff, 0xe2], 0),
        (&[0xff, 0x03, 0xff, 0xe2], 0),
        (&[0xd9, 0xe8, 0xff, 0xe2], 0),
        (&[0x8b, 0x03, 0xff, 0xe2], 1),
        (&[0xff, 0x03, 0x8b, 0x03, 0xff, 0xe2], 1),
        (&[0xd9, 0xe8, 0x8b, 0x03, 0xff, 0xe2], 1),
        (&[0x8b, 0x03, 0x8b, 0x0b, 0xff, 0xe2], 2),
        (&[0xa7, 0xff, 0xe2], 2),
        (&[0xf3, 0xa7, 0xff, 0xe2], 2),
        (&[0xff, 0x33, 0xff, 0xe2], 1),
        (&[0x8f, 0x03, 0xff, 0xe2], 1),
        (&[0xd9, 0xe8, 0x8b, 0x03, 0x8b, 0x0b, 0xff, 0xe2], 2),
        (&[0xff, 0x33, 0x8f, 0x03, 0xff, 0xe2], 2),
        (&[0xc3], 1),
    ];
    let mut modules = Vec::new();
    for (code, sites) in cases {
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
            assert_eq!(plan.imports.read32_result_sites, sites);
            assert_eq!(plan.has_read32_validator, sites >= 2);
            super::super::EMITTED_READ32_SITES.with(|count| count.set(0));
            let bytes = emit(&prepared.blocks, Some(binding), Some(cs.decode_key()));
            let emitted = super::super::EMITTED_READ32_SITES.with(|count| count.replace(0));
            assert_eq!(
                emitted, sites,
                "the planner counts real nested emission sites"
            );
            modules.push(format!("\"{}\"", hex(&bytes)));
        }
    }
    node(
        "import assert from 'node:assert/strict'; let s=''; for await(const c of process.stdin)s+=c; const xs=JSON.parse(s); assert.equal(xs.length,34); for(const [i,x]of xs.entries())assert.ok(WebAssembly.validate(Buffer.from(x,'hex')),`module ${i}`);",
        &format!("[{}]", modules.join(",")),
    );
}
