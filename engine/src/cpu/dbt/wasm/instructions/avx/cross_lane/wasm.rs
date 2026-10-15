use crate::cpu::dbt::wasm::instructions::avx::cross_lane::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemoryType, Module, TypeSection, ValType,
};
fn register(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn module(operation: CrossLaneInstruction) -> Vec<u8> {
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
    // isolated caller statuses are fixture outcomes, not guest fault ABI values.
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if let Some(r) = operation.memory() {
        code.local_get(1)
            .i32_const(65536 - i32::from(r.bytes))
            .i32_gt_u()
            .if_(BlockType::Empty)
            .i32_const(3)
            .return_()
            .end();
    }
    emit(
        &mut code,
        operation,
        VectorLayout {
            lower: 64,
            upper: 256,
        },
        CrossLaneLocals {
            state: 0,
            operand: 1,
            result: [2, 3],
        },
    );
    code.i32_const(0).end();
    let mut body = CodeSection::new();
    body.function(&function);
    module.section(&body);
    module.finish()
}
fn case(operation: CrossLaneInstruction, tag: &str, fields: &str, address: u32) -> String {
    let bytes = module(operation)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{{\"tag\":\"{tag}\",\"address\":{address},\"memoryBytes\":{},\"module\":\"{bytes}\"{fields}}}",
        operation.memory().map_or(0, |r| r.bytes)
    )
}
fn source(index: Option<u8>) -> MoveSource {
    index.map_or(MoveSource::Memory, |i| MoveSource::Register(register(i)))
}
fn fields(d: u8, a: u8, b: Option<u8>, immediate: u8) -> String {
    format!(
        ",\"destination\":{d},\"first\":{a},\"second\":{},\"immediate\":{immediate}",
        b.map_or_else(|| "\"memory\"".to_owned(), |v| v.to_string())
    )
}

#[test]
fn direct_block_movements_preserve_raw_bits_aliases_exact_spans_and_all_other_state() {
    let mut cases = Vec::new();
    for d in 0..8 {
        for (operation, width, bytes) in [
            (
                CrossLaneInstruction::Broadcast {
                    element: BroadcastElement::Dword,
                    source: MoveSource::Memory,
                    width: VectorWidth::Xmm,
                    destination: register(d),
                },
                16,
                4,
            ),
            (
                CrossLaneInstruction::Broadcast {
                    element: BroadcastElement::Dword,
                    source: MoveSource::Memory,
                    width: VectorWidth::Ymm,
                    destination: register(d),
                },
                32,
                4,
            ),
            (
                CrossLaneInstruction::Broadcast {
                    element: BroadcastElement::Qword,
                    width: VectorWidth::Ymm,
                    source: MoveSource::Memory,
                    destination: register(d),
                },
                32,
                8,
            ),
            (
                CrossLaneInstruction::BroadcastBlock {
                    destination: register(d),
                },
                32,
                16,
            ),
        ] {
            for address in [0x8003, 65536 - bytes, 65537 - bytes] {
                cases.push(case(
                    operation,
                    "broadcast",
                    &format!(",\"width\":{width},\"element\":{bytes},\"destination\":{d}"),
                    address,
                ));
            }
        }
    }
    for immediate in 0..=255_u8 {
        for (d, a, b) in [
            (0, 1, Some(2)),
            (1, 1, Some(2)),
            (2, 1, Some(2)),
            (0, 1, Some(1)),
            (0, 0, Some(0)),
            (0, 1, None),
        ] {
            cases.push(case(
                CrossLaneInstruction::Permute {
                    destination: register(d),
                    first: register(a),
                    second: source(b),
                    immediate,
                },
                "permute",
                &fields(d, a, b, immediate),
                if b.is_none() { 0x8003 } else { 0 },
            ));
        }
        for (d, a, b) in [
            (0, 1, Some(2)),
            (1, 1, Some(2)),
            (2, 1, Some(2)),
            (0, 0, Some(0)),
            (0, 1, None),
        ] {
            cases.push(case(
                CrossLaneInstruction::Insert {
                    destination: register(d),
                    first: register(a),
                    second: source(b),
                    immediate,
                },
                "insert",
                &fields(d, a, b, immediate),
                if b.is_none() { 0x8003 } else { 0 },
            ));
        }
        for destination in [Some(0), Some(1), None] {
            cases.push(case(
                CrossLaneInstruction::Extract {
                    source: register(1),
                    destination: destination
                        .map_or(BlockTarget::Memory, |r| BlockTarget::Register(register(r))),
                    immediate,
                },
                "extract",
                &format!(
                    ",\"first\":1,\"destination\":{},\"immediate\":{immediate}",
                    destination.map_or_else(|| "\"memory\"".to_owned(), |r| r.to_string())
                ),
                if destination.is_none() { 0x8003 } else { 0 },
            ));
        }
    }
    for immediate in [0, 0x88, 0xff] {
        for address in [65504, 65505] {
            cases.push(case(
                CrossLaneInstruction::Permute {
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Memory,
                    immediate,
                },
                "permute",
                &fields(0, 1, None, immediate),
                address,
            ));
        }
        for address in [65520, 65521] {
            cases.push(case(
                CrossLaneInstruction::Insert {
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Memory,
                    immediate,
                },
                "insert",
                &fields(0, 1, None, immediate),
                address,
            ));
            cases.push(case(
                CrossLaneInstruction::Extract {
                    source: register(1),
                    destination: BlockTarget::Memory,
                    immediate,
                },
                "extract",
                &format!(",\"first\":1,\"destination\":\"memory\",\"immediate\":{immediate}"),
                address,
            ));
        }
    }
    for operation in [
        CrossLaneInstruction::Broadcast {
            element: BroadcastElement::Dword,
            source: MoveSource::Memory,
            width: VectorWidth::Xmm,
            destination: register(0),
        },
        CrossLaneInstruction::Broadcast {
            element: BroadcastElement::Dword,
            source: MoveSource::Memory,
            width: VectorWidth::Ymm,
            destination: register(0),
        },
        CrossLaneInstruction::Broadcast {
            element: BroadcastElement::Qword,
            width: VectorWidth::Ymm,
            source: MoveSource::Memory,
            destination: register(0),
        },
        CrossLaneInstruction::BroadcastBlock {
            destination: register(0),
        },
        CrossLaneInstruction::Permute {
            destination: register(0),
            first: register(1),
            second: MoveSource::Memory,
            immediate: 0x88,
        },
        CrossLaneInstruction::Insert {
            destination: register(0),
            first: register(1),
            second: MoveSource::Memory,
            immediate: 1,
        },
        CrossLaneInstruction::Extract {
            source: register(1),
            destination: BlockTarget::Memory,
            immediate: 1,
        },
    ] {
        cases.push(case(
            operation,
            "invalid_state",
            ",\"invalidState\":true",
            0xffffffff,
        ));
    }
    let count = cases.len();
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
        .expect("node available for actual direct Wasm cross-lane proof");
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
        format!("{count} actual cross-lane modules passed")
    );
}
