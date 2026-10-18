use super::{ResultContext, Validation, helper_field, validate_span};
use crate::cpu::dbt::wasm::locals::{
    ADDRESS, DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, HELPER_STATUS, LHS, REASON,
    RESULT, RHS, STATE_PTR,
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const INPUTS: [u32; 12] = [
    STATE_PTR,
    HELPER_STATUS,
    EIP,
    ADDRESS,
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
    RESULT,
];
const OUTPUTS: [u32; 8] = [
    REASON,
    DETAIL,
    FAULT_ADDRESS,
    FAULT_ACCESS,
    FAULT_LENGTH,
    LHS,
    RHS,
    RESULT,
];

pub(in crate::cpu::dbt::wasm) fn call(code: &mut InstructionSink<'_>, index: u32, exit_depth: u32) {
    // preserve every caller local while selecting the unchanged slow validator.
    // only an exact ordinary success can avoid the call; its payload remains caller-validated.
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .if_(BlockType::Result(ValType::I32));
    helper_field(code, 0);
    code.i32_const(u32::from_le_bytes(*b"R3MH") as i32)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    for (position, (offset, expected)) in [
        (4, 0x10007),
        (8, 40),
        (12, 0),
        (16, 0),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 0),
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
    // the unchanged validator already performs its live-context rejection check.
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .br(exit_depth + 1)
        .end();
}

pub(in crate::cpu::dbt::wasm) fn build_function() -> Function {
    // parameters occupy 0..12; the validator uses the existing canonical locals through 26.
    // every destination is outside the parameter range, so rebinding cannot clobber an input.
    let mut function = Function::new([(ADDRESS + 1 - INPUTS.len() as u32, ValType::I32)]);
    let mut code = function.instructions();
    for (parameter, local) in INPUTS.into_iter().enumerate().skip(1) {
        code.local_get(parameter as u32).local_set(local);
    }
    code.block(BlockType::Empty);
    validate_span(
        &mut code,
        0,
        0,
        false,
        Validation::Ordinary { mask: None },
        0,
        ResultContext {
            paging_configured: true,
            paging_access_mask: None,
            zero_span_validator: None,
        },
    );
    code.end();
    for local in OUTPUTS {
        code.local_get(local);
    }
    code.end();
    function
}

#[cfg(test)]
mod tests;
