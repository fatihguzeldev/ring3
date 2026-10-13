use super::*;
use crate::cpu::x86::avx::{Element, VectorWidth};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    MemoryType, Module, TypeSection, ValType,
};
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
#[derive(Clone, Copy)]
struct Form {
    kind: &'static str,
    width: usize,
    lane: usize,
}
fn instruction(f: Form, d: u8, mask: u8, source: u8) -> MaskedInstruction {
    let width = if f.width == 16 {
        VectorWidth::Xmm
    } else {
        VectorWidth::Ymm
    };
    let element = if f.lane == 4 {
        Element::Single
    } else {
        Element::Double
    };
    match f.kind {
        "load" => MaskedInstruction::Load {
            width,
            element,
            destination: r(d),
            mask: r(mask),
        },
        "store" => MaskedInstruction::Store {
            width,
            element,
            source: r(source),
            mask: r(mask),
        },
        "bytes" => MaskedInstruction::StoreBytes {
            source: r(source),
            mask: r(mask),
        },
        _ => unreachable!(),
    }
}
fn module(i: MaskedInstruction) -> Vec<u8> {
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
    let mut function = Function::new([(6, ValType::V128)]);
    let mut code = function.instructions();
    code.local_get(0)
        .i32_load(arg(32))
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .return_()
        .end();
    emit(
        &mut code,
        i,
        VectorLayout {
            lower: 64,
            upper: 256,
        },
        MaskedLocals {
            state: 0,
            operand: 1,
            mask: [2, 3],
            data: [4, 5],
            result: [6, 7],
        },
        |code, start, size, _| {
            code.local_get(1)
                .i64_extend_i32_u()
                .i64_const(i64::from(start + size))
                .i64_add()
                .i64_const(65536)
                .i64_gt_u()
                .if_(BlockType::Empty)
                .i32_const(3)
                .return_()
                .end();
        },
    );
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}
fn case(f: Form, bits: u32, d: u8, mask: u8, source: u8, address: u32, invalid: bool) -> String {
    let i = instruction(f, d, mask, source);
    let module = module(i)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{{\"kind\":\"{}\",\"width\":{},\"lane\":{},\"maskBits\":{bits},\"d\":{d},\"mask\":{mask},\"source\":{source},\"address\":{address},\"invalid\":{invalid},\"module\":\"{module}\"}}",
        f.kind, f.width, f.lane
    )
}
#[test]
fn direct_masked_memory_matches_selected_span_and_full_memory_oracle() {
    let mut forms = vec![Form {
        kind: "bytes",
        width: 16,
        lane: 1,
    }];
    for width in [16, 32] {
        for lane in [4, 8] {
            for kind in ["load", "store"] {
                forms.push(Form { kind, width, lane });
            }
        }
    }
    let mut cases = Vec::new();
    for f in forms {
        let lanes = f.width / f.lane;
        let all = (1u32 << lanes) - 1;
        let masks: Vec<u32> = if f.kind == "bytes" {
            (0..16)
                .map(|b| 1 << b)
                .chain([0, 0xffff, 0xaaaa, 0x5555, 0x8001, 0x1ff])
                .collect()
        } else {
            (0..1u32 << lanes).collect()
        };
        for bits in masks {
            cases.push(case(f, bits, 0, 1, 2, 0x8003, false));
        }
        for bits in [0, 1, 1 << (lanes - 1), all, all & 0xaaaa] {
            let end = if bits == 0 {
                0
            } else {
                (32 - bits.leading_zeros()) as usize * f.lane
            };
            for (d, mask, source) in [(0, 1, 2), (1, 1, 2), (7, 7, 7)] {
                for address in if bits == 0 {
                    [0x8003, 65536, u32::MAX]
                } else {
                    [0x8003, 65536 - end as u32, 65537 - end as u32]
                } {
                    cases.push(case(f, bits, d, mask, source, address, false));
                }
            }
        }
        for index in 0..8 {
            cases.push(case(
                f,
                all & 0xaaaa,
                index,
                index,
                (index + 1) % 8,
                0x8003,
                false,
            ));
        }
        cases.push(case(f, all, 0, 1, 2, u32::MAX, true));
    }
    assert_eq!(cases.len(), 1092);
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
        .expect("node available for direct masked memory proof");
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
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
