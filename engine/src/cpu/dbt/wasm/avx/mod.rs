use crate::cpu::dbt::wasm::imports::Imports;
mod cross_lane;
mod direct;
mod execute;
mod gather;
mod integer;
mod locals;
mod logical;
#[cfg(test)]
mod masked;
mod moves;
mod multiply;
mod permute;
mod preflight;
mod rearrange;
mod scalar;
mod shift;

#[derive(Clone, Copy)]
pub(super) struct VectorLayout {
    pub lower: u32,
    pub upper: u32,
}

pub(super) use locals::AvxLocals;
pub(super) const INTEGER_LOCALS: u32 = 3;
pub(super) const VECTOR_LOCALS: u32 = 6;

pub(super) fn direct_hotpath(instruction: crate::cpu::x86::avx::AvxInstruction) -> bool {
    direct::accepts(instruction)
}

pub(super) fn instruction(
    code: &mut wasm_encoder::InstructionSink<'_>,
    operation: crate::cpu::x86::avx::AvxOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let locals = imports.avx_locals.expect("avx block local allocation");
    if matches!(
        operation.instruction().semantic(),
        crate::cpu::x86::avx::AvxSemantic::Gather(_)
    ) {
        gather::instruction(code, operation, imports, locals, exit_depth);
        return false;
    }
    if direct_hotpath(operation.instruction()) {
        direct::instruction(code, operation, imports, locals, exit_depth)
    } else {
        execute::instruction(code, operation, imports, locals, exit_depth)
    }
}

pub(super) fn preflight(code: &mut wasm_encoder::InstructionSink<'_>) {
    preflight::emit(code);
}

pub(super) fn initialize(code: &mut wasm_encoder::InstructionSink<'_>, locals: AvxLocals) {
    code.local_get(super::locals::STATE_PTR)
        .i32_const(crate::abi::arena::TRANSFER_OFFSET as i32)
        .i32_add()
        .local_set(locals.operand);
}
