use crate::cpu::{
    dbt::{
        region::CompiledBlock,
        wasm::{
            EmbeddedBinding,
            emitter::body,
            helpers::result::outlined,
            imports,
            instructions::{memory, segments},
            requirements::ModuleRequirements,
        },
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
    emit_with_tier(
        blocks,
        binding,
        decode_key,
        crate::cpu::dbt::CompilationTier32::Baseline,
    )
}
pub(in crate::cpu::dbt) fn emit_with_tier(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
    decode_key: Option<CodeDecodeKey>,
    tier: crate::cpu::dbt::CompilationTier32,
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
    if plan.has_zero_span_validator {
        helper_imports.zero_span_validator =
            Some(sections.run_index + 1 + u32::from(plan.has_segment_check));
        functions.function(sections.ordinary_validator_type);
    }
    if plan.has_x87_read32_validator {
        helper_imports.x87_read32_validator = Some(
            sections.run_index
                + 1
                + u32::from(plan.has_segment_check)
                + u32::from(plan.has_zero_span_validator),
        );
        functions.function(sections.ordinary_validator_type);
    }
    if plan.has_x87_write32_validator {
        helper_imports.x87_write32_validator = Some(
            sections.run_index
                + 1
                + u32::from(plan.has_segment_check)
                + u32::from(plan.has_zero_span_validator)
                + u32::from(plan.has_x87_read32_validator),
        );
        functions.function(sections.ordinary_validator_type);
    }
    if plan.has_read32_validator {
        helper_imports.scalar_read32_validator = Some(
            sections.run_index
                + 1
                + u32::from(plan.has_segment_check)
                + u32::from(plan.has_zero_span_validator)
                + u32::from(plan.has_x87_read32_validator)
                + u32::from(plan.has_x87_write32_validator),
        );
        functions.function(sections.scalar_read32_validator_type);
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
    let function = body::emit(blocks, binding, &plan, &mut helper_imports, tier);
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    if plan.has_segment_check {
        bodies.function(&segments::build_function());
    }
    if plan.has_zero_span_validator {
        bodies.function(&outlined::build_function());
    }
    if plan.has_x87_read32_validator {
        bodies.function(&outlined::build_read32_function());
    }
    if plan.has_x87_write32_validator {
        bodies.function(&outlined::build_write32_function());
    }
    if plan.has_read32_validator {
        bodies.function(&memory::build_read32_validator());
    }
    module.section(&bodies);
    module.finish()
}
