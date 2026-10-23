use super::{BACKING, BACKING_PAGES, MAPPINGS, PERMISSIONS, arg};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(in crate::cpu::core) const TRIED: u32 = 132;
pub(in crate::cpu::core) const VALID: u32 = 133;
pub(in crate::cpu::core) const ADMITTED: u32 = 150;

pub(in crate::cpu::core) fn anchor(slot: u8) -> u32 {
    134 + u32::from(slot) * 2
}

pub(in crate::cpu::core) fn physical_slot(slot: u8) -> u32 {
    anchor(slot) + 1
}

pub(in crate::cpu::core) fn invalidate(code: &mut InstructionSink<'_>) {
    for local in [TRIED, VALID] {
        code.i32_const(0).local_set(local);
    }
}

// Pure admission: context, logical/linear current address, current offset from
// envelope start, envelope length, rights, logical maximum, alignment mask/residue.
// Results: physical slot, physical envelope anchor, admitted. Failure observes no
// guest memory and leaves the current instruction's ordinary access authoritative.
pub(in crate::cpu::core) fn emit_acquire() -> Function {
    let mut function = Function::new([(4, ValType::I32)]);
    let mut code = function.instructions();
    code.block(BlockType::Empty)
        .local_get(1)
        .local_get(3)
        .i32_lt_u()
        .br_if(0)
        .local_get(1)
        .local_get(3)
        .i32_sub()
        .local_tee(9)
        .local_get(6)
        .local_get(4)
        .i32_sub()
        .i32_const(1)
        .i32_add()
        .i32_gt_u()
        .br_if(0)
        .local_get(2)
        .local_get(3)
        .i32_sub()
        .local_tee(10)
        .i32_const(4095)
        .i32_and()
        .i32_const(4096)
        .local_get(4)
        .i32_sub()
        .i32_gt_u()
        .br_if(0)
        .local_get(10)
        .local_get(7)
        .i32_and()
        .local_get(8)
        .i32_ne()
        .br_if(0)
        .local_get(0)
        .i32_load(arg(MAPPINGS, 2))
        .local_get(10)
        .i32_const(12)
        .i32_shr_u()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .i32_const(1)
        .i32_sub()
        .local_tee(11)
        .local_get(0)
        .i32_load(arg(BACKING_PAGES, 2))
        .i32_ge_u()
        .br_if(0)
        .local_get(0)
        .i32_load(arg(PERMISSIONS, 2))
        .local_get(10)
        .i32_const(12)
        .i32_shr_u()
        .i32_add()
        .i32_load8_u(arg(0, 0))
        .local_tee(12)
        .local_get(5)
        .i32_and()
        .local_get(5)
        .i32_ne()
        .br_if(0)
        .local_get(11)
        .local_get(0)
        .i32_load(arg(BACKING, 2))
        .local_get(11)
        .i32_const(12)
        .i32_shl()
        .i32_add()
        .local_get(10)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .i32_const(1)
        .return_()
        .end()
        .i32_const(0)
        .i32_const(0)
        .i32_const(0)
        .end();
    function
}
