use super::body;
use crate::cpu::dbt::wasm::requirements::ModuleRequirements;
use crate::cpu::{
    dbt::{
        region::CompiledBlock,
        wasm::{EmbeddedBinding, imports, segments},
    },
    x86::decode::CodeDecodeKey,
};
use wasm_encoder::{
    CodeSection, ConstExpr, ExportKind, ExportSection, FunctionSection, GlobalSection, GlobalType,
    Module, ValType,
};

pub(in crate::cpu::dbt) fn emit(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
    decode_key: Option<CodeDecodeKey>,
) -> Vec<u8> {
    let plan = ModuleRequirements::new(blocks, binding);
    let sections = imports::wiring::build(&plan, binding);
    let mut module = Module::new();
    module.section(&sections.types);
    module.section(&sections.imports);
    let mut helper_imports = sections.context;
    let mut functions = FunctionSection::new();
    functions.function(0);
    if plan.has_segment_check {
        helper_imports.segment_check = Some(sections.run_index + 1);
        functions.function(sections.segment_check_type);
    }
    module.section(&functions);
    let key = decode_key.unwrap_or(crate::cpu::x86::decode::CodeDecodeKey::FLAT);
    let mut globals = GlobalSection::new();
    for value in [
        i32::from(binding.is_some()),
        key.cs_base as i32,
        i32::from(key.default32),
    ] {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(value),
        );
    }

    module.section(&globals);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, sections.run_index);
    module.section(&exports);
    let function = body::emit(blocks, binding, &plan, &mut helper_imports);
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    if plan.has_segment_check {
        bodies.function(&segments::build_function());
    }
    module.section(&bodies);
    module.finish()
}
