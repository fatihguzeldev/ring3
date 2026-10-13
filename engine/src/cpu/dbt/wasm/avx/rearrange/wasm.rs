use super::*;
use crate::cpu::x86::simd::{RearrangeOperation, ShuffleKind as S};
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
fn forms() -> Vec<RearrangeOperation> {
    let mut out = Vec::new();
    for kind in [
        S::UnpackLow,
        S::UnpackHigh,
        S::PackSigned,
        S::PackUnsigned,
        S::Dwords,
        S::HighWords,
        S::LowWords,
        S::Bytes,
        S::AlignRight,
        S::BlendImmediate,
        S::BlendVariable,
        S::SignExtendBytes,
        S::SignExtendWords,
        S::SignExtendDwords,
        S::ZeroExtendBytes,
        S::ZeroExtendWords,
        S::ZeroExtendDwords,
    ] {
        for width in [1, 2, 4, 8] {
            if let Some(op) = RearrangeOperation::new(kind, width, 0) {
                out.push(op);
            }
        }
    }
    assert_eq!(out.len(), 31);
    out
}
fn module(instruction: RearrangeInstruction) -> Vec<u8> {
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
    let mut function = Function::new([(3, ValType::V128)]);
    let mut code = function.instructions();
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if let Some(requirement) = instruction.memory() {
        code.local_get(1)
            .i32_const(65536 - i32::from(requirement.bytes))
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
        RearrangeLocals {
            state: 0,
            operand: 1,
            sources: [2, 3, 4],
        },
    );
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}
fn case(
    op: RearrangeOperation,
    d: u8,
    a: u8,
    b: Option<u8>,
    m: u8,
    seed: u8,
    address: u32,
    invalid: bool,
) -> String {
    let instruction = RearrangeInstruction::new(
        op,
        r(d),
        (!op.unary()).then_some(r(a)),
        b.map_or(MoveSource::Memory, |i| MoveSource::Register(r(i))),
        op.uses_mask().then_some(r(m)),
    )
    .unwrap();
    let bytes = module(instruction)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{{\"kind\":\"{:?}\",\"width\":{},\"immediate\":{},\"d\":{d},\"a\":{a},\"b\":{},\"mask\":{m},\"seed\":{seed},\"address\":{address},\"invalid\":{invalid},\"memoryBytes\":{},\"module\":\"{bytes}\"}}",
        op.kind(),
        op.lane_bytes(),
        op.immediate(),
        b.map_or_else(|| "\"memory\"".to_owned(), |i| i.to_string()),
        instruction.memory().map_or(0, |r| r.bytes)
    )
}

#[test]
fn direct_rearrangements_match_independent_full_memory_oracle() {
    let mut cases = Vec::new();
    for op in forms() {
        for seed in [0, 3, 7] {
            for (d, a, b, m) in [
                (0, 1, Some(2), 3),
                (0, 0, Some(2), 3),
                (0, 1, Some(0), 3),
                (0, 1, Some(1), 3),
                (0, 0, Some(0), 0),
                (0, 1, Some(2), 0),
                (0, 1, None, 3),
            ] {
                if b.is_some() {
                    cases.push(case(op, d, a, b, m, seed, 0, false));
                } else {
                    for address in [
                        0x8003,
                        65536 - u32::from(op.memory_bytes()),
                        65537 - u32::from(op.memory_bytes()),
                    ] {
                        cases.push(case(op, d, a, b, m, seed, address, false));
                    }
                }
            }
        }
        if matches!(
            op.kind(),
            S::Dwords | S::HighWords | S::LowWords | S::AlignRight | S::BlendImmediate
        ) {
            for immediate in 0..=255 {
                let op = RearrangeOperation::new(op.kind(), op.lane_bytes(), immediate).unwrap();
                cases.push(case(op, 0, 1, Some(2), 3, 5, 0, false));
            }
        }
        cases.push(case(op, 0, 1, Some(2), 3, 0, 0, true));
    }
    assert_eq!(cases.len(), 2148);
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
        .expect("node available for direct AVX packed proof");
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
        format!("{} actual rearrange modules passed", cases.len())
    );
}
