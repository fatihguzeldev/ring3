use super::*;
use crate::cpu::x86::simd::{PackedKind as P, ShiftOperation};
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
fn module(i: ShiftInstruction) -> Vec<u8> {
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
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if i.memory().is_some() {
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
        i,
        VectorLayout {
            lower: 64,
            upper: 256,
        },
        ShiftLocals {
            state: 0,
            operand: 1,
            sources: [2, 3],
        },
    );
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}
fn case(
    op: ShiftOperation,
    immediate: Option<u8>,
    (d, a, b): (u8, u8, Option<u8>),
    count: u64,
    seed: u8,
    address: u32,
    invalid: bool,
) -> String {
    let i = ShiftInstruction::new(
        op,
        r(d),
        r(a),
        immediate.map_or_else(
            || b.map_or(ShiftCount::Memory, |i| ShiftCount::Register(r(i))),
            ShiftCount::Immediate,
        ),
    )
    .unwrap();
    let module = module(i)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let kind = op.lane_kind().map_or_else(
        || format!("{:?}", op.byte_kind().unwrap()),
        |k| format!("{k:?}"),
    );
    format!(
        "{{\"kind\":\"{kind}\",\"width\":{},\"immediate\":{},\"d\":{d},\"a\":{a},\"b\":{},\"count\":\"{count}\",\"seed\":{seed},\"address\":{address},\"invalid\":{invalid},\"memoryBytes\":{},\"module\":\"{module}\"}}",
        op.lane_bytes(),
        immediate.map_or_else(|| "null".to_owned(), |i| i.to_string()),
        b.map_or_else(|| "\"memory\"".to_owned(), |i| i.to_string()),
        i.memory().map_or(0, |r| r.bytes)
    )
}
#[test]
fn direct_shifts_match_full_count_and_full_memory_oracle() {
    let mut cases = Vec::new();
    let mut lane_forms = Vec::new();
    for kind in [P::ShiftLeft, P::ShiftRight, P::ShiftRightSigned] {
        for width in [2, 4, 8] {
            if let Some(op) = ShiftOperation::lanes(kind, width) {
                lane_forms.push(op);
            }
        }
    }
    assert_eq!(lane_forms.len(), 8);
    let mut immediate_forms = lane_forms.clone();
    immediate_forms.extend([ShiftOperation::bytes(false), ShiftOperation::bytes(true)]);
    for op in immediate_forms {
        for n in 0..=255 {
            cases.push(case(op, Some(n), (0, 1, Some(2)), 0, 3, 0, false));
        }
        for (d, a) in [(0, 0), (7, 7), (7, 1)] {
            for n in [0, 1, 15, 16, 31, 32, 63, 64, 255] {
                cases.push(case(op, Some(n), (d, a, Some(2)), 0, 5, 0, false));
            }
        }
        cases.push(case(op, Some(0), (0, 1, Some(2)), 0, 0, 0, true));
    }
    for op in lane_forms {
        for count in [
            0,
            1,
            15,
            16,
            31,
            32,
            63,
            64,
            65,
            255,
            1u64 << 32,
            1u64 << 63,
            u64::MAX,
        ] {
            for seed in [0, 3, 7] {
                for (d, a, b) in [
                    (0, 1, Some(2)),
                    (0, 0, Some(2)),
                    (0, 1, Some(0)),
                    (0, 1, Some(1)),
                    (0, 0, Some(0)),
                    (0, 1, None),
                ] {
                    if b.is_some() {
                        cases.push(case(op, None, (d, a, b), count, seed, 0, false));
                    } else {
                        for address in [0x8003, 65520, 65521] {
                            cases.push(case(op, None, (d, a, b), count, seed, address, false));
                        }
                    }
                }
            }
        }
        cases.push(case(op, None, (0, 1, Some(2)), 0, 0, 0, true));
    }
    assert_eq!(cases.len(), 5344);
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
        .expect("node available for direct AVX shift proof");
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
        format!("{} actual shift modules passed", cases.len())
    );
}
