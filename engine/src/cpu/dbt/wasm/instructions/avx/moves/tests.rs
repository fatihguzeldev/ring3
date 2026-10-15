use crate::cpu::dbt::wasm::instructions::avx::moves::*;
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
fn module(operation: MoveInstruction) -> Vec<u8> {
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
    let mut function = Function::new([(2, ValType::V128), (1, ValType::I32)]);
    let mut code = function.instructions();
    // isolated caller preflight models the boundary required by the kernel. these
    // status values are fixture outcomes, not Ring3's guest exit ABI or #GP codes.
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    if let Some(requirement) = operation.memory() {
        if requirement.alignment > 1 {
            code.local_get(1)
                .i32_const(i32::from(requirement.alignment) - 1)
                .i32_and()
                .if_(BlockType::Empty)
                .i32_const(2)
                .return_()
                .end();
        }
        code.local_get(1)
            .i32_const(65536 - i32::from(requirement.bytes))
            .i32_gt_u()
            .if_(BlockType::Empty)
            .i32_const(3)
            .return_()
            .end();
    }
    code.block(BlockType::Empty);
    emit(
        &mut code,
        operation,
        MoveLayout {
            vectors: VectorLayout {
                lower: 64,
                upper: 256,
            },
            general: 0,
            mxcsr: 32,
        },
        MoveLocals {
            state: 0,
            operand: 1,
            result: [2, 3],
            control: 4,
        },
        0,
    );
    code.i32_const(0).return_().end().i32_const(1).end();
    let mut body = CodeSection::new();
    body.function(&function);
    module.section(&body);
    module.finish()
}

fn case(operation: MoveInstruction, tag: &str, fields: &str, address: u32) -> String {
    let bytes = module(operation)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let memory = operation.memory();
    format!(
        "{{\"tag\":\"{tag}\",\"address\":{address},\"memoryBytes\":{},\"alignment\":{},\"module\":\"{bytes}\"{fields}}}",
        memory.map_or(0, |r| r.bytes),
        memory.map_or(1, |r| r.alignment)
    )
}

#[test]
fn direct_movement_wasm_preserves_exact_spans_aliases_and_upper_rules() {
    let mut cases = Vec::new();
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for source in [register(0), register(1)] {
            cases.push(case(
                MoveInstruction::Copy {
                    width,
                    destination: register(0),
                    source,
                },
                "copy",
                &format!(
                    ",\"width\":{},\"source\":{},\"destination\":0",
                    width.bytes(),
                    source.index()
                ),
                0,
            ));
        }
        for aligned in [false, true] {
            for load in [false, true] {
                let operation = if load {
                    MoveInstruction::Load {
                        width,
                        destination: register(0),
                        aligned,
                    }
                } else {
                    MoveInstruction::Store {
                        width,
                        source: register(1),
                        aligned,
                    }
                };
                for address in [
                    if aligned { 0x8000 } else { 0x8003 },
                    65536 - width.bytes() as u32,
                ] {
                    cases.push(case(
                        operation,
                        if load { "load" } else { "store" },
                        &format!(
                            ",\"width\":{},\"source\":1,\"destination\":0,\"aligned\":{aligned}",
                            width.bytes()
                        ),
                        address,
                    ));
                }
                if aligned && width == VectorWidth::Ymm {
                    cases.push(case(
                        operation,
                        if load { "load" } else { "store" },
                        ",\"width\":32,\"source\":1,\"destination\":0,\"aligned\":true",
                        0x8010,
                    ));
                }
                let address = if aligned {
                    0x8001
                } else {
                    65536 - width.bytes() as u32 + 1
                };
                cases.push(case(
                    operation,
                    if load { "load" } else { "store" },
                    &format!(
                        ",\"width\":{},\"source\":1,\"destination\":0,\"aligned\":{aligned}",
                        width.bytes()
                    ),
                    address,
                ));
            }
        }
    }
    for (destination, merge, source) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
        for element in [Element::Single, Element::Double] {
            cases.push(case(MoveInstruction::Scalar { element, destination: register(destination), merge: register(merge), source: register(source) }, "scalar", &format!(",\"element\":{},\"destination\":{destination},\"merge\":{merge},\"source\":{source}", element.bytes()), 0));
        }
        for (kind, tag) in [
            (HalfMerge::HighToLow, "high_to_low"),
            (HalfMerge::LowToHigh, "low_to_high"),
        ] {
            cases.push(case(
                MoveInstruction::MergeHalf {
                    kind,
                    destination: register(destination),
                    merge: register(merge),
                    source: register(source),
                },
                tag,
                &format!(",\"destination\":{destination},\"merge\":{merge},\"source\":{source}"),
                0,
            ));
        }
    }
    for element in [Element::Single, Element::Double] {
        for load in [false, true] {
            let operation = if load {
                MoveInstruction::LoadScalar {
                    element,
                    destination: register(0),
                }
            } else {
                MoveInstruction::StoreScalar {
                    element,
                    source: register(1),
                }
            };
            for address in [
                0x8003,
                65536 - element.bytes() as u32,
                65536 - element.bytes() as u32 + 1,
            ] {
                cases.push(case(
                    operation,
                    if load { "load_scalar" } else { "store_scalar" },
                    &format!(
                        ",\"element\":{},\"destination\":0,\"source\":1",
                        element.bytes()
                    ),
                    address,
                ));
            }
        }
    }
    for half in [Half::Low, Half::High] {
        for load in [false, true] {
            let operation = if load {
                MoveInstruction::LoadHalf {
                    half,
                    destination: register(0),
                    merge: register(0),
                }
            } else {
                MoveInstruction::StoreHalf {
                    half,
                    source: register(1),
                }
            };
            for address in [0x8003, 65528, 65529] {
                cases.push(case(
                    operation,
                    if load { "load_half" } else { "store_half" },
                    &format!(
                        ",\"half\":{},\"destination\":0,\"merge\":0,\"source\":1",
                        if half == Half::Low { 0 } else { 8 }
                    ),
                    address,
                ));
            }
        }
    }
    for (kind, tag) in [
        (DuplicateKind::SingleLow, "duplicate_single_low"),
        (DuplicateKind::SingleHigh, "duplicate_single_high"),
        (DuplicateKind::DoubleLow, "duplicate_double_low"),
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for source in [
                MoveSource::Register(register(0)),
                MoveSource::Register(register(1)),
                MoveSource::Memory,
            ] {
                let operation = MoveInstruction::Duplicate {
                    kind,
                    width,
                    destination: register(0),
                    source,
                };
                let field = match source {
                    MoveSource::Register(r) => format!(",\"source\":{}", r.index()),
                    MoveSource::Memory => ",\"source\":\"memory\"".to_owned(),
                };
                let addresses = if let Some(r) = operation.memory() {
                    vec![
                        0x8003,
                        65536 - u32::from(r.bytes),
                        65536 - u32::from(r.bytes) + 1,
                    ]
                } else {
                    vec![0]
                };
                for address in addresses {
                    cases.push(case(
                        operation,
                        tag,
                        &format!(",\"width\":{},\"destination\":0{field}", width.bytes()),
                        address,
                    ));
                }
            }
        }
    }
    for general in [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ] {
        cases.push(case(
            MoveInstruction::DwordLoad {
                destination: register(0),
                source: WordSource::Register(general),
            },
            "load_dword",
            &format!(",\"destination\":0,\"source\":{}", general.index()),
            0,
        ));
        cases.push(case(
            MoveInstruction::DwordStore {
                source: register(1),
                destination: WordTarget::Register(general),
            },
            "store_dword",
            &format!(",\"source\":1,\"destination\":{}", general.index()),
            0,
        ));
        for element in [Element::Single, Element::Double] {
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                cases.push(case(
                    MoveInstruction::Mask {
                        element,
                        width,
                        destination: general,
                        source: register(1),
                    },
                    "mask",
                    &format!(
                        ",\"element\":{},\"width\":{},\"destination\":{},\"source\":1",
                        element.bytes(),
                        width.bytes(),
                        general.index()
                    ),
                    0,
                ));
            }
        }
    }
    for load in [false, true] {
        let operation = if load {
            MoveInstruction::DwordLoad {
                destination: register(0),
                source: WordSource::Memory,
            }
        } else {
            MoveInstruction::DwordStore {
                source: register(1),
                destination: WordTarget::Memory,
            }
        };
        for address in [0x8003, 65532, 65533] {
            cases.push(case(
                operation,
                if load { "load_dword" } else { "store_dword" },
                if load {
                    ",\"destination\":0,\"source\":\"memory\""
                } else {
                    ",\"destination\":\"memory\",\"source\":1"
                },
                address,
            ));
        }
    }
    for source in [
        MoveSource::Register(register(0)),
        MoveSource::Register(register(1)),
        MoveSource::Memory,
    ] {
        let operation = MoveInstruction::QwordLoad {
            destination: register(0),
            source,
        };
        let field = match source {
            MoveSource::Register(r) => format!(",\"source\":{}", r.index()),
            MoveSource::Memory => ",\"source\":\"memory\"".to_owned(),
        };
        let addresses = if source == MoveSource::Memory {
            vec![0x8003, 65528, 65529]
        } else {
            vec![0]
        };
        for address in addresses {
            cases.push(case(
                operation,
                "load_qword",
                &format!(",\"destination\":0{field}"),
                address,
            ));
        }
    }
    for address in [0x8003, 65528, 65529] {
        cases.push(case(
            MoveInstruction::QwordStore {
                source: register(1),
            },
            "store_qword",
            ",\"source\":1",
            address,
        ));
    }
    for control in [0_u32, 0x1f80, 0xffff, 0x1_0000, 0x8000_1f80] {
        cases.push(case(
            MoveInstruction::LoadControl,
            "load_control",
            &format!(",\"control\":{control}"),
            0x8003,
        ));
    }
    cases.push(case(
        MoveInstruction::StoreControl,
        "store_control",
        "",
        65532,
    ));
    cases.push(case(
        MoveInstruction::LoadControl,
        "load_control",
        ",\"control\":8064",
        65533,
    ));
    cases.push(case(
        MoveInstruction::StoreControl,
        "store_control",
        "",
        65533,
    ));
    cases.push(case(MoveInstruction::ZeroUpper, "zero_upper", "", 0));
    cases.push(case(MoveInstruction::ZeroAll, "zero_all", "", 0));
    cases.push(case(
        MoveInstruction::ZeroAll,
        "zero_all",
        ",\"invalidState\":true",
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
        .expect("node available for actual Wasm movement proof");
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
        format!("{count} actual movement modules passed")
    );
}
