use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, X87_OFFSET},
        x86::X87_SIZE,
    },
    process::EngineInstance,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use wasm_encoder::{
    CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    GlobalSection, GlobalType, ImportSection, MemArg, MemoryType, Module, TypeSection, ValType,
};

fn malformed_helper_injector() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    for parameters in [1, 2, 6] {
        types
            .ty()
            .function(vec![ValType::I32; parameters], [ValType::I32]);
    }
    types.ty().function([ValType::I32], []);
    types.ty().function([], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
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
    module.section(&imports);
    let mut functions = FunctionSection::new();
    for index in [3, 0, 1, 2, 4, 4, 4] {
        functions.function(index);
    }
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for _ in 0..4 {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
    }
    module.section(&globals);
    let mut exports = ExportSection::new();
    for (index, name) in [
        "configure",
        "read16",
        "store16",
        "store_resident16",
        "calls",
        "address",
        "value",
    ]
    .into_iter()
    .enumerate()
    {
        exports.export(name, ExportKind::Func, index as u32);
    }
    module.section(&exports);
    let mut code = CodeSection::new();
    let mut configure = Function::new([]);
    configure.instructions().local_get(0).global_set(0);
    for index in 1..4 {
        configure.instructions().i32_const(0).global_set(index);
    }
    configure.instructions().end();
    code.function(&configure);
    for (version, address, value) in [(2, 0, None), (4, 0, Some(1)), (4, 4, Some(5))] {
        let mut helper = Function::new([]);
        let mut body = helper.instructions();
        body.global_get(1)
            .i32_const(1)
            .i32_add()
            .global_set(1)
            .local_get(address)
            .global_set(2);
        if let Some(value) = value {
            body.local_get(value).global_set(3);
        }
        for (index, value) in [
            0x484d_3352,
            0x0001_0000 | version,
            40,
            0,
            0,
            if version == 2 { 0x8001 } else { 0 },
            0,
            0,
            0,
            1,
        ]
        .into_iter()
        .enumerate()
        {
            body.global_get(0).i32_const(value).i32_store(MemArg {
                offset: (index * 4) as u64,
                align: 2,
                memory_index: 0,
            });
        }
        body.i32_const(0).end();
        code.function(&helper);
    }
    for index in 1..4 {
        let mut accessor = Function::new([]);
        accessor.instructions().global_get(index).end();
        code.function(&accessor);
    }
    module.section(&code);
    module.finish()
}

fn write_new(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn actual_engine_word_memory_moves_atomic_faults_current_ram_and_currency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let parent = root.join("target/p2-word-memory-move-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&output).unwrap();
    let injector = output.join("malformed-helper.wasm");
    write_new(&injector, &malformed_helper_injector());
    write_new(
        &output.join("cases.json"),
        include_bytes!("fixtures/p2-word-memory-move/cases.json"),
    );
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(ARENA_SIZE, 4364);
    assert_eq!(X87_SIZE, 128);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    write_new(&output.join("initial-arena.bin"), &initial);
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-memory-move/run.mjs"))
        .arg(&engine)
        .arg(&injector)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "word memory MOV integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
