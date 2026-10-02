use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, InstructionSink, MemoryType, Module, TypeSection, ValType,
};

use super::{EmbeddedBinding, abi, integer, locals::*, memory};
use crate::cpu::dbt::region::CompiledBlock;

pub(in crate::cpu::dbt) fn emit(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
) -> Vec<u8> {
    let mut module = Module::new();
    let (needs_read, needs_store, needs_read8, needs_read16) = memory::Imports::needed(blocks);
    let has_reads = needs_read || needs_read8 || needs_read16;
    let has_memory = has_reads || needs_store;
    let has_gates = blocks.iter().any(|block| block.gate.is_some());
    debug_assert!(!(has_memory || has_gates) || binding.is_some());
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    if let Some(binding) = binding {
        let parameters = match binding {
            EmbeddedBinding::Replacement { .. } => 6,
            EmbeddedBinding::Resident { .. } => 7,
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    if has_reads {
        types.ty().function([ValType::I32], [ValType::I32]);
    }
    if needs_store {
        types.ty().function([ValType::I32; 2], [ValType::I32]);
    }
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    if let Some(binding) = binding {
        let name = match binding {
            EmbeddedBinding::Replacement { .. } => "guard",
            EmbeddedBinding::Resident { .. } => "guard_resident",
        };
        imports.import("ring3", name, EntityType::Function(1));
    }
    let mut helper_imports = memory::Imports::default();
    let mut function_index = u32::from(binding.is_some());
    let mut type_index = 1 + u32::from(binding.is_some());
    let read_type_index = type_index;
    if needs_read {
        imports.import("ring3", "read32", EntityType::Function(type_index));
        helper_imports.read = Some(function_index);
        function_index += 1;
        type_index += 1;
    } else if has_reads {
        type_index += 1;
    }
    if needs_store {
        imports.import("ring3", "store32", EntityType::Function(type_index));
        helper_imports.store = Some(function_index);
        function_index += 1;
    }
    if needs_read8 {
        imports.import("ring3", "read8", EntityType::Function(read_type_index));
        helper_imports.read8 = Some(function_index);
        function_index += 1;
    }
    if needs_read16 {
        imports.import("ring3", "read16", EntityType::Function(read_type_index));
        helper_imports.read16 = Some(function_index);
        function_index += 1;
    }
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, function_index);
    module.section(&exports);
    let mut declarations = vec![(16, ValType::I32), (1, ValType::I64)];
    if has_memory || has_gates {
        declarations.push((6, ValType::I32));
    }
    let mut function = Function::new(declarations);
    let mut code = function.instructions();
    if let Some(binding) = binding {
        match binding {
            EmbeddedBinding::Replacement { key, generation } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(generation as i32);
            }
            EmbeddedBinding::Resident { key, id } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(id as u32 as i32)
                    .i32_const((id >> 32) as u32 as i32);
            }
        }
        code.local_get(STATE_PTR)
            .local_get(EXIT_PTR)
            .local_get(CANCEL_PTR)
            .call(0)
            .local_tee(LHS)
            .if_(BlockType::Empty)
            .local_get(LHS)
            .return_()
            .end();
    }
    abi::preflight(&mut code);
    abi::load_state(&mut code);
    code.block(BlockType::Empty).loop_(BlockType::Empty);
    abi::safepoint(&mut code, 1);
    for block in blocks {
        emit_block(&mut code, block, helper_imports);
    }
    code.i32_const(3).local_set(REASON).br(1).end().end();
    abi::flush(&mut code, has_memory, has_gates);
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}

fn emit_block(code: &mut InstructionSink<'_>, block: &CompiledBlock, imports: memory::Imports) {
    if let Some(gate) = &block.gate {
        code.local_get(EIP)
            .i32_const(gate.entry.0 as i32)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(gate.id as i32)
            .local_set(DETAIL)
            .i32_const(8)
            .local_set(REASON)
            .br(2)
            .end();
        return;
    }
    code.i32_const(-1).local_set(RESUME);
    for (index, instruction) in block.instructions.iter().enumerate() {
        code.local_get(EIP)
            .i32_const(instruction.pc().0 as i32)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(index as i32)
            .local_set(RESUME)
            .end();
    }
    code.local_get(RESUME)
        .i32_const(-1)
        .i32_ne()
        .if_(BlockType::Empty);
    let count = block.instructions.len() as u32;
    for _ in 0..count {
        code.block(BlockType::Empty);
    }
    // nested labels resume inside a block, then execution falls through its remaining instructions.
    code.local_get(RESUME).br_table(0..count, count);
    for (index, instruction) in block.instructions.iter().enumerate() {
        code.end();
        let remaining = count - index as u32 - 1;
        abi::safepoint(code, remaining + 2);
        integer::instruction(code, instruction, imports, remaining + 2);
    }
    code.br(1).end();
}
