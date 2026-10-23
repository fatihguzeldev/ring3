use super::super::mem;
use crate::cpu::core::fp_pool as p;
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

fn record_pointer(code: &mut InstructionSink<'_>, destination: u32) {
    code.local_get(0)
        .local_get(1)
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(destination);
}

/// The caller supplies a certified native value and a physical slot in 0..8.
/// This commits one payload; the caller owns tag, status and changed-mask effects.
pub(in crate::cpu::core::emitter) fn emit_native_writer() -> Function {
    let mut function = Function::new([(1, ValType::I32)]);
    let mut code = function.instructions();
    record_pointer(&mut code, 5);
    code.local_get(5)
        .local_get(2)
        .f64_store(mem(p::NATIVE, 3))
        .local_get(5)
        .i32_const(1)
        .i32_store(mem(p::VALID, 2))
        .local_get(5)
        .local_get(3)
        .i32_const(7)
        .i32_and()
        .i32_store(mem(p::PROOFS, 2))
        .local_get(5)
        .local_get(4)
        .i32_store(mem(p::PROOF_CONTROL, 2))
        .local_get(2)
        .f64_const(0.0.into())
        .f64_eq()
        .end();
    function
}

/// Opaque writes preserve all raw payload bits, including empty physical ghosts.
pub(in crate::cpu::core::emitter) fn emit_raw_writer() -> Function {
    let mut function = Function::new([(1, ValType::I32)]);
    let mut code = function.instructions();
    record_pointer(&mut code, 4);
    code.local_get(4)
        .local_get(2)
        .i64_store(mem(p::LOW, 3))
        .local_get(4)
        .local_get(3)
        .i32_store(mem(p::HIGH, 2));
    for offset in [p::VALID, p::PROOFS, p::PROOF_CONTROL] {
        code.local_get(4).i32_const(0).i32_store(mem(offset, 2));
    }
    code.local_get(2)
        .i64_eqz()
        .local_get(3)
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_()
        .local_get(3)
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u()
        .local_get(2)
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end()
        .end();
    function
}
