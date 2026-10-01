use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, InstructionSink, MemoryType, Module, TypeSection, ValType,
};

use super::{abi, integer, locals::*};
use crate::cpu::dbt::region::CompiledBlock;

pub(in crate::cpu::dbt) fn emit(blocks: &[CompiledBlock]) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
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
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([(16, ValType::I32), (1, ValType::I64)]);
    let mut code = function.instructions();
    abi::preflight(&mut code);
    abi::load_state(&mut code);
    code.block(BlockType::Empty).loop_(BlockType::Empty);
    abi::safepoint(&mut code, 1);
    for block in blocks {
        emit_block(&mut code, block);
    }
    code.i32_const(3).local_set(REASON).br(1).end().end();
    abi::flush(&mut code);
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    module.section(&bodies);
    module.finish()
}

fn emit_block(code: &mut InstructionSink<'_>, block: &CompiledBlock) {
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
        integer::instruction(code, instruction);
    }
    code.br(1).end();
}
