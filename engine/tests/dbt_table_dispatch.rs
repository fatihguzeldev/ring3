use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::{BlockSpec, CompileLimits, compile_region};
use ring3_engine::memory::{AddressSpace, GuestAddress, PageRange, Permissions};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    InstructionSink, MemArg, MemoryType, Module, RefType, TableType, TypeSection, ValType,
};

const TOTAL: u32 = 4;
const PC: u32 = 5;
const SLOT: u32 = 6;
const STATUS: u32 = 7;
const CHILD_RETIRED: u32 = 8;
const REASON: u32 = 9;

fn memarg(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

fn load(code: &mut InstructionSink<'_>, pointer: u32, offset: u64) {
    code.local_get(pointer).i32_load(memarg(offset));
}

fn finish(code: &mut InstructionSink<'_>, reason: Option<i32>) {
    for (offset, value) in [
        (0, i32::from_le_bytes(*b"R3EX")),
        (4, 0x10001),
        (8, 40),
        (12, 0),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 0),
    ] {
        code.local_get(1).i32_const(value).i32_store(memarg(offset));
    }
    code.local_get(1);
    if let Some(reason) = reason {
        code.i32_const(reason);
    } else {
        code.local_get(REASON);
    }
    code.i32_store(memarg(16))
        .local_get(1)
        .local_get(TOTAL)
        .i32_store(memarg(20))
        .i32_const(0)
        .return_();
}

fn exit_if(code: &mut InstructionSink<'_>, reason: i32) {
    code.if_(BlockType::Empty);
    finish(code, Some(reason));
    code.end();
}

// this experiment accepts only the harness's fixed valid buffers; it is not a public dispatcher abi.
fn dispatcher() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 5], [ValType::I32]);
    module.section(&types);
    let mut imports = wasm_encoder::ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import(
        "env",
        "table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 2,
            maximum: Some(2),
            shared: false,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0).function(1);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports
        .export("run", ExportKind::Func, 0)
        .export("probe", ExportKind::Func, 1);
    module.section(&exports);
    let mut run = Function::new([(6, ValType::I32)]);
    let mut code = run.instructions();
    code.i32_const(240)
        .i32_const(0)
        .i32_store(memarg(0))
        .loop_(BlockType::Empty);
    load(&mut code, 3, 0);
    exit_if(&mut code, 2);
    code.local_get(2).i32_eqz();
    exit_if(&mut code, 1);
    load(&mut code, 0, 48);
    code.local_set(PC).i32_const(-1).local_set(SLOT);
    for (pc, slot) in [(0x1000, 0), (0x1001, 0), (0x2000, 1), (0x2001, 1)] {
        code.local_get(PC)
            .i32_const(pc)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(slot)
            .local_set(SLOT)
            .end();
    }
    code.local_get(SLOT).i32_const(-1).i32_eq();
    exit_if(&mut code, 3);
    code.i32_const(244)
        .i32_load(memarg(0))
        .i32_const(1)
        .local_get(SLOT)
        .i32_shl()
        .i32_and()
        .i32_eqz();
    exit_if(&mut code, 3);
    code.local_get(SLOT).table_size(0).i32_ge_u();
    exit_if(&mut code, 3);
    code.local_get(SLOT).table_get(0).ref_is_null();
    exit_if(&mut code, 3);
    code.i32_const(240)
        .i32_const(240)
        .i32_load(memarg(0))
        .i32_const(1)
        .i32_add()
        .i32_store(memarg(0));
    code.local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .local_get(SLOT)
        .call_indirect(0, 0)
        .local_tee(STATUS)
        .if_(BlockType::Empty)
        .local_get(STATUS)
        .return_()
        .end();
    load(&mut code, 1, 20);
    code.local_set(CHILD_RETIRED)
        .local_get(TOTAL)
        .local_get(CHILD_RETIRED)
        .i32_add()
        .local_set(TOTAL)
        .local_get(2)
        .local_get(CHILD_RETIRED)
        .i32_sub()
        .local_set(2);
    load(&mut code, 1, 16);
    code.local_tee(REASON)
        .i32_const(3)
        .i32_ne()
        .if_(BlockType::Empty);
    finish(&mut code, None);
    code.end().local_get(CHILD_RETIRED).i32_eqz();
    exit_if(&mut code, 3);
    code.br(0).end().i32_const(0).end();
    let mut probe = Function::new([]);
    probe
        .instructions()
        .local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .local_get(4)
        .call_indirect(0, 0)
        .end();
    let mut bodies = CodeSection::new();
    bodies.function(&run).function(&probe);
    module.section(&bodies);
    module.finish()
}

fn wrong_signature() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    module.section(&types);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("wrong", ExportKind::Func, 0);
    module.section(&exports);
    let mut body = Function::new([]);
    body.instructions().i32_const(0).end();
    let mut code = CodeSection::new();
    code.function(&body);
    module.section(&code);
    module.finish()
}

#[test]
fn standalone_regions_share_a_wasm_table_without_warm_host_calls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-table-dispatch-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let mut memory = AddressSpace::new(2).unwrap();
    for (name, pc, bytes) in [
        ("a", 0x1000, &[0x40, 0xe9, 0xfa, 0x0f, 0, 0][..]),
        ("b", 0x2000, &[0x49, 0x0f, 0x85, 0xf9, 0xef, 0xff, 0xff][..]),
    ] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), bytes).unwrap();
        let compiled = compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap();
        assert_eq!(compiled.metadata().instructions, 2);
        fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("{name}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    fs::write(output.join("dispatcher.wasm"), dispatcher()).unwrap();
    fs::write(output.join("wrong.wasm"), wrong_signature()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-table-dispatch/run.mjs"))
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "table experiment failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
