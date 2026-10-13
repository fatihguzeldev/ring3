use super::*;
use crate::cpu::x86::Register32;
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
fn module(operation: PermuteInstruction) -> Vec<u8> {
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
        PermuteLayout {
            vectors: VectorLayout {
                lower: 64,
                upper: 256,
            },
            general: 0,
        },
        PermuteLocals {
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
fn case(operation: PermuteInstruction, tag: &str, fields: &str, address: u32) -> String {
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
fn fields(
    element: Element,
    width: VectorWidth,
    d: u8,
    a: u8,
    b: Option<u8>,
    immediate: u8,
) -> String {
    format!(
        ",\"element\":{},\"width\":{},\"destination\":{d},\"first\":{a},\"second\":{},\"immediate\":{immediate}",
        element.bytes(),
        width.bytes(),
        b.map_or_else(|| "\"memory\"".to_owned(), |v| v.to_string())
    )
}
#[test]
fn direct_permutations_preserve_lane_boundaries_aliases_masks_and_exact_spans() {
    let mut cases = Vec::new();
    for element in [Element::Single, Element::Double] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for (d, a, b) in [
                (0, 1, Some(2)),
                (0, 0, Some(2)),
                (0, 1, Some(0)),
                (0, 1, Some(1)),
                (0, 0, Some(0)),
                (0, 0, None),
            ] {
                let second = source(b);
                let addresses = if b.is_none() {
                    vec![
                        0x8003,
                        65536 - width.bytes() as u32,
                        65537 - width.bytes() as u32,
                    ]
                } else {
                    vec![0]
                };
                for address in addresses {
                    for high in [false, true] {
                        cases.push(case(
                            PermuteInstruction::Interleave {
                                high,
                                element,
                                width,
                                destination: register(d),
                                first: register(a),
                                second,
                            },
                            if high { "unpack_high" } else { "unpack_low" },
                            &fields(element, width, d, a, b, 0),
                            address,
                        ));
                    }
                    for immediate in [0, 0x1b, 0x39, 0x96, 0xe4, 0xff] {
                        cases.push(case(
                            PermuteInstruction::Shuffle {
                                element,
                                width,
                                destination: register(d),
                                first: register(a),
                                second,
                                immediate,
                            },
                            "shuffle",
                            &fields(element, width, d, a, b, immediate),
                            address,
                        ));
                        cases.push(case(
                            PermuteInstruction::Immediate {
                                element,
                                width,
                                destination: register(d),
                                source: second,
                                immediate,
                            },
                            "immediate",
                            &fields(element, width, d, a, b, immediate),
                            address,
                        ));
                        cases.push(case(
                            PermuteInstruction::Blend {
                                element,
                                width,
                                destination: register(d),
                                first: register(a),
                                second,
                                immediate,
                            },
                            "blend",
                            &fields(element, width, d, a, b, immediate),
                            address,
                        ));
                    }
                    for pattern in 0..4 {
                        cases.push(case(
                            PermuteInstruction::Variable {
                                element,
                                width,
                                destination: register(d),
                                source: register(a),
                                control: second,
                            },
                            "variable",
                            &format!(
                                "{},\"pattern\":{pattern}",
                                fields(element, width, d, a, b, 0)
                            ),
                            address,
                        ));
                    }
                }
            }
            for (d, a, b, mask) in [
                (0, 1, Some(2), 3),
                (0, 0, Some(2), 3),
                (0, 1, Some(0), 3),
                (0, 1, Some(2), 0),
                (0, 1, Some(2), 1),
                (0, 1, Some(2), 2),
                (0, 0, Some(0), 0),
                (0, 1, None, 0),
            ] {
                let addresses = if b.is_none() {
                    vec![
                        0x8003,
                        65536 - width.bytes() as u32,
                        65537 - width.bytes() as u32,
                    ]
                } else {
                    vec![0]
                };
                for address in addresses {
                    for pattern in 0..4 {
                        cases.push(case(
                            PermuteInstruction::BlendVariable {
                                element,
                                width,
                                destination: register(d),
                                first: register(a),
                                second: source(b),
                                mask: register(mask),
                            },
                            "blend_variable",
                            &format!(
                                "{},\"mask\":{mask},\"pattern\":{pattern}",
                                fields(element, width, d, a, b, 0)
                            ),
                            address,
                        ));
                    }
                }
            }
        }
    }
    for (d, a, b) in [
        (0, 1, Some(2)),
        (0, 0, Some(2)),
        (0, 1, Some(0)),
        (0, 1, Some(1)),
        (0, 0, Some(0)),
        (0, 0, None),
    ] {
        for immediate in [0, 0x10, 0x23, 0x39, 0x5a, 0x96, 0xe4, 0xff] {
            let addresses = if b.is_none() {
                vec![0x8003, 65532, 65533]
            } else {
                vec![0]
            };
            for address in addresses {
                cases.push(case(
                    PermuteInstruction::InsertSingle {
                        destination: register(d),
                        first: register(a),
                        second: source(b),
                        immediate,
                    },
                    "insert",
                    &fields(Element::Single, VectorWidth::Xmm, d, a, b, immediate),
                    address,
                ));
            }
        }
    }
    for (destination, index) in [
        (Register32::Eax, 0),
        (Register32::Ecx, 1),
        (Register32::Edx, 2),
        (Register32::Ebx, 3),
        (Register32::Esp, 4),
        (Register32::Ebp, 5),
        (Register32::Esi, 6),
        (Register32::Edi, 7),
    ] {
        for immediate in [0, 1, 2, 3, 0xfc, 0xfd, 0xfe, 0xff] {
            cases.push(case(
                PermuteInstruction::ExtractSingle {
                    source: register(1),
                    destination: WordTarget::Register(destination),
                    immediate,
                },
                "extract",
                &format!(",\"destination\":{index},\"first\":1,\"immediate\":{immediate}"),
                0,
            ));
        }
    }
    for immediate in [0, 1, 2, 3, 0xfc, 0xfd, 0xfe, 0xff] {
        for address in [0x8003, 65532, 65533] {
            cases.push(case(
                PermuteInstruction::ExtractSingle {
                    source: register(1),
                    destination: WordTarget::Memory,
                    immediate,
                },
                "extract",
                &format!(",\"destination\":\"memory\",\"first\":1,\"immediate\":{immediate}"),
                address,
            ));
        }
    }
    cases.push(case(
        PermuteInstruction::Immediate {
            element: Element::Double,
            width: VectorWidth::Ymm,
            destination: register(0),
            source: MoveSource::Register(register(1)),
            immediate: 0,
        },
        "immediate",
        ",\"element\":8,\"width\":32,\"destination\":0,\"first\":1,\"second\":1,\"immediate\":0,\"invalidState\":true",
        0,
    ));
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
        .expect("node available for direct AVX permutation proof");
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
        format!("{count} actual permutation modules passed")
    );
}
