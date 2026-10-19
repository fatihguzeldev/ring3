use super::{ResultContext, Validation, helper_field, validate_span};
use crate::cpu::dbt::wasm::locals::{
    ADDRESS, DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, HELPER_STATUS, LHS, REASON,
    RESULT, RHS, STATE_PTR,
};
use wasm_encoder::{BlockType, Function, InstructionSink, MemArg, ValType};

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

#[cfg(test)]
thread_local! {
    static EMITTED_READ32_SITES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static EMITTED_WRITE32_SITES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn record_read32_site() {
    EMITTED_READ32_SITES.with(|sites| sites.set(sites.get() + 1));
}

#[cfg(test)]
fn take_read32_sites() -> usize {
    EMITTED_READ32_SITES.with(|sites| sites.replace(0))
}

#[cfg(test)]
pub(super) fn record_write32_site() {
    EMITTED_WRITE32_SITES.with(|sites| sites.set(sites.get() + 1));
}

#[cfg(test)]
fn take_write32_sites() -> usize {
    EMITTED_WRITE32_SITES.with(|sites| sites.replace(0))
}

pub(in crate::cpu::dbt::wasm) fn call(code: &mut InstructionSink<'_>, index: u32, exit_depth: u32) {
    call_span(code, index, exit_depth, 0);
}

pub(in crate::cpu::dbt::wasm) fn call_read32(
    code: &mut InstructionSink<'_>,
    index: u32,
    exit_depth: u32,
) {
    call_span(code, index, exit_depth, 4);
}

pub(in crate::cpu::dbt::wasm) fn call_write32(
    code: &mut InstructionSink<'_>,
    index: u32,
    exit_depth: u32,
) {
    call_span(code, index, exit_depth, 4);
}

fn call_span(code: &mut InstructionSink<'_>, index: u32, exit_depth: u32, length: i32) {
    // preserve every caller local while selecting the unchanged slow validator.
    // only an exact ordinary success can avoid the call; its payload remains caller-validated.
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .if_(BlockType::Result(ValType::I32));
    helper_pair(
        code,
        0,
        (0x10007_i64 << 32) | i64::from(u32::from_le_bytes(*b"R3MH")),
    );
    code.if_(BlockType::Result(ValType::I32));
    helper_pair(code, 8, 40);
    helper_field(code, 16);
    code.i32_eqz().i32_and();
    for (offset, expected) in [(24, 0), (32, i64::from(length) << 32)] {
        helper_pair(code, offset, expected);
        code.i32_and();
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

fn helper_pair(code: &mut InstructionSink<'_>, offset: u64, expected: i64) {
    code.local_get(STATE_PTR)
        .i64_load(MemArg {
            offset: 100 + offset,
            align: 2,
            memory_index: 0,
        })
        .i64_const(expected)
        .i64_eq();
}

pub(in crate::cpu::dbt::wasm) fn build_function() -> Function {
    build_span_function(0, 0, false)
}

pub(in crate::cpu::dbt::wasm) fn build_read32_function() -> Function {
    build_span_function(4, 1, false)
}

pub(in crate::cpu::dbt::wasm) fn build_write32_function() -> Function {
    build_span_function(4, 2, true)
}

fn build_span_function(length: u32, access: u32, allow_smc: bool) -> Function {
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
        length,
        access,
        allow_smc,
        Validation::Ordinary { mask: None },
        0,
        ResultContext {
            paging_configured: true,
            paging_access_mask: None,
            zero_span_validator: None,
            x87_read32_validator: None,
            x87_write32_validator: None,
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
