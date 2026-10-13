use super::*;
use crate::cpu::x86::simd::{IntegerLaneOperation, PackedKind as P};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemoryType, Module, TypeSection, ValType,
};

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn module(instruction: IntegerInstruction) -> Vec<u8> {
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
    let mut function = Function::new([(2, ValType::V128)]);
    let mut code = function.instructions();
    // local fixture outcomes do not define guest fault or exit packet ordering.
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if instruction.memory().is_some() {
        code.local_get(1)
            .i32_const(65520)
            .i32_gt_u()
            .if_(BlockType::Empty)
            .i32_const(3)
            .return_()
            .end();
    }
    emit(
        &mut code,
        instruction,
        VectorLayout {
            lower: 64,
            upper: 256,
        },
        IntegerLocals {
            state: 0,
            operand: 1,
            scratch: [2, 3],
        },
    );
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}

fn case(
    operation: IntegerLaneOperation,
    d: u8,
    a: u8,
    b: Option<u8>,
    seed: u8,
    address: u32,
    invalid: bool,
) -> String {
    let instruction = IntegerInstruction::new(
        operation,
        register(d),
        (!operation.unary()).then_some(register(a)),
        b.map_or(MoveSource::Memory, |r| MoveSource::Register(register(r))),
    )
    .unwrap();
    let module = module(instruction)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{{\"kind\":\"{:?}\",\"width\":{},\"destination\":{d},\"first\":{a},\"second\":{},\"seed\":{seed},\"address\":{address},\"invalidState\":{invalid},\"memoryBytes\":{},\"module\":\"{module}\"}}",
        operation.kind(),
        operation.lane_bytes(),
        b.map_or_else(|| "\"memory\"".to_owned(), |register| register.to_string()),
        instruction.memory().map_or(0, |r| r.bytes)
    )
}

#[test]
fn direct_integer_lanes_match_independent_full_memory_oracle() {
    let mut cases = Vec::new();
    let mut forms = 0;
    for kind in [
        P::Add,
        P::Subtract,
        P::AddSignedSaturate,
        P::AddUnsignedSaturate,
        P::SubtractSignedSaturate,
        P::SubtractUnsignedSaturate,
        P::MinimumSigned,
        P::MaximumSigned,
        P::MinimumUnsigned,
        P::MaximumUnsigned,
        P::AverageUnsigned,
        P::Equal,
        P::GreaterSigned,
        P::Absolute,
        P::Sign,
    ] {
        for width in [1, 2, 4, 8] {
            let Some(operation) = IntegerLaneOperation::new(kind, width) else {
                continue;
            };
            forms += 1;
            for seed in [0, 3, 6] {
                for (d, a, b) in [
                    (0, 1, Some(2)),
                    (0, 0, Some(2)),
                    (0, 1, Some(0)),
                    (0, 1, Some(1)),
                    (0, 0, Some(0)),
                    (0, 0, None),
                ] {
                    let addresses = if b.is_some() {
                        vec![0]
                    } else {
                        vec![0x8003, 65520, 65521]
                    };
                    for address in addresses {
                        cases.push(case(operation, d, a, b, seed, address, false));
                    }
                }
            }
        }
    }
    assert_eq!(forms, 44);
    cases.push(case(
        IntegerLaneOperation::new(P::Sign, 4).unwrap(),
        0,
        1,
        Some(2),
        0,
        0,
        true,
    ));
    let count = cases.len();
    assert_eq!(count, 1057);
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
        .expect("node available for direct AVX integer proof");
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
        format!("{count} actual integer modules passed")
    );
}
