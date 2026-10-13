use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    MemoryType, Module, TypeSection, ValType,
};

fn module(operation: LogicalInstruction) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], []);
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
    emit(
        &mut function.instructions(),
        operation,
        VectorLayout {
            lower: 64,
            upper: 256,
        },
        0,
        0,
        [1, 2],
    );
    function.instructions().end();
    let mut code = CodeSection::new();
    code.function(&function);
    module.section(&code);
    module.finish()
}

#[test]
fn direct_logical_wasm_preserves_aliases_upper_rules_and_unrelated_bytes() {
    let mut cases = Vec::new();
    for (kind, tag) in [
        (LogicalOperation::And, "and"),
        (LogicalOperation::AndNot, "and_not"),
        (LogicalOperation::Or, "or"),
        (LogicalOperation::Xor, "xor"),
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for (destination, first, second) in
                [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)]
            {
                let operation = LogicalInstruction {
                    kind,
                    width,
                    destination: VectorRegister::new(destination).unwrap(),
                    first: VectorRegister::new(first).unwrap(),
                    second: MoveSource::Register(VectorRegister::new(second).unwrap()),
                };
                let bytes = module(operation)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                cases.push(format!("{{\"kind\":\"{tag}\",\"width\":{},\"destination\":{destination},\"first\":{first},\"second\":{second},\"module\":\"{bytes}\"}}", width.bytes()));
            }
        }
    }
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
        .expect("node available for actual Wasm kernel proof");
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
        "40 actual logical modules passed"
    );
}
