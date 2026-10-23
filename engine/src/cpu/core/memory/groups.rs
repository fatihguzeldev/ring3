use super::{BACKING, BACKING_PAGES, EXECUTABLE_VIEWS, MAPPINGS, PERMISSIONS, PRIVATE_SLOTS, arg};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(in crate::cpu::core) const TRIED: u32 = 132;
pub(in crate::cpu::core) const VALID: u32 = 133;
pub(in crate::cpu::core) const ADMITTED: u32 = 150;
pub(in crate::cpu::core) const GEOMETRY: i32 = 1;
pub(in crate::cpu::core) const PLAIN_WRITE: i32 = 1 << 8;

pub(in crate::cpu::core) fn anchor(slot: u8) -> u32 {
    134 + u32::from(slot) * 2
}

pub(in crate::cpu::core) fn physical_slot(slot: u8) -> u32 {
    anchor(slot) + 1
}

pub(in crate::cpu::core) fn invalidate(code: &mut InstructionSink<'_>) {
    for local in [TRIED, VALID, ADMITTED] {
        code.i32_const(0).local_set(local);
    }
}

// Pure admission: context, logical/linear current address, current offset from
// envelope start, envelope length, rights, logical maximum, alignment mask/residue.
// results: physical slot, physical envelope anchor, capability flags. the low
// bit admits geometry; bit8 additionally proves nonexec physical bytes and a
// noncow virtual page. data transactions cannot change either authority fact.
// failed write-authority speculation still admits ordinary read geometry.
pub(in crate::cpu::core) fn emit_acquire() -> Function {
    let mut function = Function::new([(5, ValType::I32)]);
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
        .i32_const(GEOMETRY)
        .local_set(12)
        .local_get(5)
        .i32_const(2)
        .i32_and()
        .if_(BlockType::Empty)
        .block(BlockType::Empty)
        .local_get(0)
        .i32_load(arg(EXECUTABLE_VIEWS, 2))
        .local_get(11)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(0)
        .block(BlockType::Empty)
        .local_get(0)
        .i32_load(arg(PRIVATE_SLOTS, 2))
        .local_tee(13)
        .i32_eqz()
        .br_if(0)
        .local_get(13)
        .local_get(10)
        .i32_const(12)
        .i32_shr_u()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(1)
        .end()
        .i32_const(GEOMETRY | PLAIN_WRITE)
        .local_set(12)
        .end()
        .end()
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
        .local_get(12)
        .return_()
        .end()
        .i32_const(0)
        .i32_const(0)
        .i32_const(0)
        .end();
    function
}
