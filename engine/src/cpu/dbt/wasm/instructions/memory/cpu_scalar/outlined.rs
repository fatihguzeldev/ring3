use super::{CPU_SCALAR_HELPER_VERSION, CpuScalarOperation, helper_field, validate};
use crate::cpu::dbt::wasm::locals::{
    ADDRESS, DETAIL, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, HELPER_STATUS, LHS, REASON, RHS,
    STATE_PTR,
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const INPUTS: [u32; 10] = [
    STATE_PTR,
    HELPER_STATUS,
    ADDRESS,
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
];
const OUTPUTS: [u32; 7] = [
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
];

pub(in crate::cpu::dbt::wasm) fn call(code: &mut InstructionSink<'_>, index: u32) {
    // the hot predicate preserves every local until the complete read32 success is known.
    // the returned payload is unrestricted; the caller consumes it after its exit checks.
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .if_(BlockType::Result(ValType::I32));
    helper_field(code, 0);
    code.i32_const(u32::from_le_bytes(*b"R3MH") as i32)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    for (position, (offset, expected)) in [
        (4, 0x10000 | i32::from(CPU_SCALAR_HELPER_VERSION)),
        (8, 40),
        (12, 0),
        (16, 0),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 4),
    ]
    .into_iter()
    .enumerate()
    {
        helper_field(code, offset);
        code.i32_const(expected).i32_eq();
        if position != 0 {
            code.i32_and();
        }
    }
    code.else_().i32_const(0).end().else_().i32_const(0).end();
    code.if_(BlockType::Empty)
        .i32_const(0)
        .local_set(LHS)
        .i32_const(0)
        .local_set(RHS)
        .else_();
    for local in INPUTS {
        code.local_get(local);
    }
    code.call(index);
    for local in OUTPUTS.into_iter().rev() {
        code.local_set(local);
    }
    code.end();
}

pub(in crate::cpu::dbt::wasm) fn build_function() -> Function {
    // the canonical validator's destinations are all outside the ten parameter slots.
    let mut function = Function::new([(ADDRESS + 1 - INPUTS.len() as u32, ValType::I32)]);
    let mut code = function.instructions();
    for (parameter, local) in INPUTS.into_iter().enumerate().skip(1) {
        code.local_get(parameter as u32).local_set(local);
    }
    validate(&mut code, 4, CpuScalarOperation::Read, true);
    for local in OUTPUTS {
        code.local_get(local);
    }
    code.end();
    function
}

#[cfg(test)]
mod tests;
