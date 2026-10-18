use crate::cpu::{
    dbt::{
        CompilationTier32,
        region::CompiledBlock,
        wasm::{
            EmbeddedBinding, abi, code_fetch,
            imports::Imports,
            instructions::{integer, x87},
            locals::*,
            requirements::ModuleRequirements,
        },
    },
    x86::ir::Operation,
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};
pub(super) fn emit(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
    plan: &ModuleRequirements,
    helper_imports: &mut Imports,
    tier: CompilationTier32,
) -> Function {
    let mut declarations = vec![(16, ValType::I32), (1, ValType::I64), (10, ValType::I32)];
    if blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(instruction.operation(), Operation::Simd(operation)
            if operation.kind == crate::cpu::x86::simd::SimdKind::Crypto(
                crate::cpu::x86::simd::CryptoKind::CarrylessMultiply))
        })
    {
        declarations.push((5, ValType::I64));
    }
    if binding.is_some() {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.code_fetch_limit = Some(first);
        declarations.push((1, ValType::I32));
    }
    if blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction.operation(), Operation::InterruptReturn { .. }))
    {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.interrupt_return_locals =
            Some(crate::cpu::dbt::wasm::instructions::far::InterruptReturnLocals::new(first));
        declarations.push((3, ValType::I32));
    }

    if plan.has_avx {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.avx_locals = Some(crate::cpu::dbt::wasm::instructions::avx::AvxLocals::new(
            first,
        ));
        declarations.push((
            crate::cpu::dbt::wasm::instructions::avx::INTEGER_LOCALS,
            ValType::I32,
        ));
        declarations.push((
            crate::cpu::dbt::wasm::instructions::avx::VECTOR_LOCALS,
            ValType::V128,
        ));
    }
    if plan.detects_exceptions {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.detection = Some(crate::cpu::dbt::wasm::detection::DetectionLocals::new(
            first,
            plan.has_repeat,
        ));
        declarations.push((1, ValType::I64));
        declarations.push((6, ValType::I32));
    }

    helper_imports.paging_configured = plan.paging_configured;
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
            .call(0);
        let detection = helper_imports
            .detection
            .expect("bound module has detection locals");

        code.local_tee(detection.epoch)
            .i64_const(0)
            .i64_le_s()
            .if_(BlockType::Empty)
            .local_get(detection.epoch)
            .i64_const(-22)
            .i64_ge_s()
            .local_get(detection.epoch)
            .i64_const(0)
            .i64_lt_s()
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .local_get(detection.epoch)
            .i64_const(-1)
            .i64_mul()
            .i32_wrap_i64()
            .else_()
            .i32_const(9)
            .end()
            .return_()
            .end();
    }
    abi::preflight(&mut code);
    if plan.has_segments {
        crate::cpu::dbt::wasm::instructions::segments::preflight(&mut code);
    }
    if plan.has_repeat {
        abi::work_preflight(&mut code);
        code.local_get(BUDGET).local_set(ORIGINAL_BUDGET);
    }
    if plan.has_x87 {
        x87::preflight(&mut code);
    }
    if plan.has_simd {
        crate::cpu::dbt::wasm::instructions::simd::preflight(&mut code);
    }
    if plan.has_avx || plan.upper_xstate {
        crate::cpu::dbt::wasm::instructions::avx::preflight(&mut code);
    }
    abi::load_state(&mut code);
    if let Some(limit) = helper_imports.code_fetch_limit {
        code_fetch::refresh(&mut code, limit);
    }
    if let Some(locals) = helper_imports.avx_locals {
        crate::cpu::dbt::wasm::instructions::avx::initialize(&mut code, locals);
    }
    code.block(BlockType::Empty);

    code.loop_(BlockType::Empty);
    abi::safepoint(&mut code, 1);
    if binding.is_some() {
        code_fetch::check_context(&mut code, 1);
    }
    for block in blocks {
        emit_block(&mut code, block, *helper_imports, tier);
    }
    if let Some(limit) = helper_imports.code_fetch_limit {
        code_fetch::check_pc(&mut code, limit, 1);
    }
    code.i32_const(3).local_set(REASON).br(1).end().end();
    if plan.has_repeat {
        code.local_get(STATE_PTR)
            .local_get(ORIGINAL_BUDGET)
            .local_get(BUDGET)
            .i32_sub()
            .i32_store(wasm_encoder::MemArg {
                offset: crate::abi::arena::WORK_CONSUMED_OFFSET as u64,
                align: 2,
                memory_index: 0,
            });
    }
    abi::flush(
        &mut code,
        plan.has_memory,
        plan.has_gates,
        plan.has_arithmetic,
        plan.has_cpu
            || plan.needs_fetch_paging
            || plan.has_simd
            || plan.has_reads
            || plan.has_stores
            || plan.imports.compare_exchange32
            || plan.imports.compare_exchange8
            || plan.imports.compare_exchange16,
        plan.has_xstate,
        plan.paging_configured,
    );
    crate::cpu::dbt::wasm::detection::finish(&mut code, *helper_imports);
    code.i32_const(0);
    code.end();
    function
}

fn emit_block(
    code: &mut InstructionSink<'_>,
    block: &CompiledBlock,
    imports: Imports,
    tier: CompilationTier32,
) {
    if let Some(gate) = block.gate() {
        code.local_get(EIP)
            .i32_const(gate.entry.0 as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        if let Some(limit) = imports.code_fetch_limit {
            code_fetch::check_context(code, 2);
            code_fetch::refresh(code, limit);
            code_fetch::check_span(code, gate.entry.0, 2, limit, 2);
        }
        code.i32_const(gate.id as i32)
            .local_set(DETAIL)
            .i32_const(8)
            .local_set(REASON)
            .br(2)
            .end();
        return;
    }
    if !block.instructions.is_empty() {
        let indexed = (tier == CompilationTier32::Optimized)
            .then(|| block.indexed_span())
            .flatten();
        if let Some((base, span)) = indexed {
            code.local_get(EIP)
                .i32_const(base as i32)
                .i32_sub()
                .local_tee(RESUME)
                .i32_const(span as i32)
                .i32_lt_u()
                .if_(BlockType::Empty);
        } else {
            code.i32_const(-1).local_set(RESUME).block(BlockType::Empty);
            for (index, instruction) in block.instructions.iter().enumerate() {
                code.local_get(EIP)
                    .i32_const(instruction.pc().0 as i32)
                    .i32_eq()
                    .if_(BlockType::Empty)
                    .i32_const(index as i32)
                    .local_set(RESUME)
                    .br(1)
                    .end();
            }
            code.end()
                .local_get(RESUME)
                .i32_const(-1)
                .i32_ne()
                .if_(BlockType::Empty);
        }
        if let Some(limit) = imports.code_fetch_limit {
            // the loop's safepoint and key check already precede this selected block.
            code_fetch::refresh(code, limit);
        }
        let count = block.instructions.len() as u32;
        for _ in 0..count {
            code.block(BlockType::Empty);
        }
        if let Some((base, span)) = indexed {
            let mut entries = vec![count; span as usize];
            for (index, instruction) in block.instructions.iter().enumerate() {
                entries[(instruction.pc().0 - base) as usize] = index as u32;
            }
            code.local_get(RESUME).br_table(entries, count);
        } else {
            code.local_get(RESUME).br_table(0..count, count);
        }
        for (index, instruction) in block.instructions.iter().enumerate() {
            code.end();
            let remaining = count - index as u32 - 1;
            abi::safepoint(code, remaining + 2);
            if let Some(limit) = imports.code_fetch_limit {
                if index != 0
                    && code_fetch::refresh_after(block.instructions[index - 1].operation())
                {
                    code_fetch::check_context(code, remaining + 2);
                    code_fetch::refresh(code, limit);
                }
                code_fetch::check_instruction(code, instruction, limit, remaining + 2);
            }
            x87::precheck_instruction(code, instruction, imports, remaining + 2);
            crate::cpu::dbt::wasm::detection::snapshot(code, instruction, imports);
            abi::begin_instruction(code);
            if matches!(instruction.operation(), Operation::InterruptReturn { .. }) {
                abi::interrupt_return_precheck(code, remaining + 2);
            }
            integer::instruction(code, instruction, imports, remaining + 2);
            if matches!(
                instruction.operation(),
                Operation::FarJump { .. }
                    | Operation::FarCall { .. }
                    | Operation::FarReturn { .. }
                    | Operation::SoftwareInterrupt { .. }
            ) && let Some(limit) = imports.code_fetch_limit
            {
                code_fetch::refresh(code, limit);
            }
            abi::debug_after_retirement(code, remaining + 2, imports.detection, false);
        }
        code.br(1).end();
    }
    if let Some(fault) = block.context_fault() {
        code.local_get(EIP)
            .i32_const(fault.pc.0 as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        abi::safepoint(code, 2);
        code_fetch::check_context(code, 2);
        code_fetch::refresh(
            code,
            imports
                .code_fetch_limit
                .expect("context fault requires a bound module"),
        );
        code_fetch::check_span(
            code,
            fault.pc.0,
            fault.attempted_length,
            imports
                .code_fetch_limit
                .expect("context fault requires a bound module"),
            2,
        );
        if fault.kind == crate::cpu::x86::decode::CapturedFetchFaultKind32::Paging {
            code_fetch::paging_fault(code, fault, imports);
        } else {
            code.i32_const(3).local_set(REASON);
        }
        code.br(2).end();
    }
}
