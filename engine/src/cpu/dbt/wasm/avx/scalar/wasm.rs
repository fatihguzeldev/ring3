use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemoryType, Module, TypeSection, ValType,
};
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn g(i: u8) -> Register32 {
    [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ][usize::from(i)]
}
fn module(i: ScalarInstruction) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
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
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([(1, ValType::V128), (1, ValType::I32)]);
    let mut code = function.instructions();
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if let Some(req) = i.memory() {
        code.local_get(1)
            .i32_const(65536 - i32::from(req.bytes))
            .i32_gt_u()
            .if_(BlockType::Empty)
            .i32_const(3)
            .return_()
            .end();
    }
    emit(
        &mut code,
        i,
        ScalarLayout {
            vectors: VectorLayout {
                lower: 64,
                upper: 256,
            },
            general: 0,
        },
        ScalarLocals {
            state: 0,
            operand: 1,
            vector: 2,
            scalar: 3,
        },
    );
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}
fn case(
    form: &str,
    width: ScalarWidth,
    imm: u8,
    d: u8,
    a: u8,
    scalar: u8,
    memory: bool,
    address: u32,
    invalid: bool,
) -> String {
    let i = match form {
        "insert" => ScalarInstruction::Insert {
            width,
            destination: r(d),
            merge: r(a),
            source: if memory {
                WordSource::Memory
            } else {
                WordSource::Register(g(scalar))
            },
            immediate: imm,
        },
        "extract" | "legacyWord" => ScalarInstruction::Extract {
            width,
            source: r(a),
            destination: if memory {
                WordTarget::Memory
            } else {
                WordTarget::Register(g(scalar))
            },
            immediate: imm,
        },
        "mask" => ScalarInstruction::Mask {
            source: r(a),
            destination: g(scalar),
        },
        _ => unreachable!(),
    };
    assert!(!(form == "legacyWord" && memory));
    let bytes = module(i)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{{\"form\":\"{form}\",\"width\":{},\"immediate\":{imm},\"d\":{d},\"a\":{a},\"scalar\":{scalar},\"memory\":{memory},\"address\":{address},\"invalid\":{invalid},\"memoryBytes\":{},\"write\":{},\"module\":\"{bytes}\"}}",
        width.bytes(),
        i.memory().map_or(0, |r| r.bytes),
        i.memory()
            .is_some_and(|r| r.access == crate::memory::Access::Write)
    )
}
#[test]
fn direct_scalar_transfers_match_exact_span_gpr_and_full_memory_oracle() {
    let mut cases = Vec::new();
    let forms = [
        ("insert", ScalarWidth::Byte),
        ("insert", ScalarWidth::Word),
        ("insert", ScalarWidth::Dword),
        ("extract", ScalarWidth::Byte),
        ("extract", ScalarWidth::Word),
        ("extract", ScalarWidth::Dword),
        ("legacyWord", ScalarWidth::Word),
        ("mask", ScalarWidth::Byte),
    ];
    for (form, width) in forms {
        for imm in 0..=if form == "mask" { 0 } else { 255 } {
            cases.push(case(form, width, imm, 0, 1, 2, false, 0, false));
        }
        if form == "insert" {
            for (d, a, g) in [
                (0, 0, 0),
                (0, 1, 0),
                (7, 7, 7),
                (7, 0, 7),
                (7, 1, 0),
                (1, 7, 1),
                (0, 7, 7),
            ] {
                for imm in [0, 1, 3, 7, 15, 16, 31, 128, 255] {
                    cases.push(case(form, width, imm, d, a, g, false, 0, false));
                }
            }
        }
        if form != "legacyWord" && form != "mask" {
            for imm in [0, 1, 3, 7, 15, 16, 31, 128, 255] {
                for address in [
                    0x8003,
                    65536 - u32::from(width.bytes()),
                    65537 - u32::from(width.bytes()),
                ] {
                    cases.push(case(form, width, imm, 0, 1, 2, true, address, false));
                }
            }
        }
        if form != "insert" {
            for a in 0..8 {
                for g in 0..8 {
                    cases.push(case(form, width, 255, 0, a, g, false, 0, false));
                }
            }
        }
        cases.push(case(form, width, 0, 0, 1, 2, false, 0, true));
    }
    assert_eq!(cases.len(), 2472);
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
        .expect("node available for direct AVX scalar proof");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", cases.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("{} actual scalar modules passed", cases.len())
    );
}
