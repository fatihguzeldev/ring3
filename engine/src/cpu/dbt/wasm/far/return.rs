use super::super::segments;
use super::*;
use crate::cpu::x86::{Register32, SegmentAccess, SegmentRegister};

pub(in crate::cpu::dbt::wasm) fn return_(
    code: &mut InstructionSink<'_>,
    width: FarWidth,
    stack_adjust: u16,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let slot = width.slot_bytes();
    let esp = register(Register32::Esp);
    // keep the two logical slots separate: B32 permits wrapping between slots,
    // while each individual slot must satisfy the checked cached span policy.
    for offset in [0, slot] {
        code.local_get(esp)
            .i32_const(offset as i32)
            .i32_add()
            .local_set(ADDRESS);
        segments::check_span(
            code,
            SegmentRegister::Ss,
            slot,
            SegmentAccess::Read,
            imports,
            exit_depth,
        );
    }
    for offset in [0, slot] {
        code.local_get(esp).i32_const(offset as i32).i32_add();
        segments::base(code, SegmentRegister::Ss);
        code.i32_add()
            .local_tee(ADDRESS)
            .i32_const(slot as i32)
            .i32_const(1)
            .call(imports.cpu_read.expect("far return stack read"))
            .local_set(HELPER_STATUS);
        cpu_result::validate(code, slot, 1, false, exit_depth);
        code.local_get(STATE_PTR);
        if offset == 0 {
            match width {
                FarWidth::Word => {
                    code.i32_load16_u(transfer(0));
                }
                FarWidth::Dword => {
                    code.i32_load(transfer(0));
                }
            }
            code.local_set(RESULT);
        } else {
            // a 32-bit return pops the whole slot but discards its upper selector bits.
            code.i32_load16_u(transfer(0)).local_set(ADDRESS);
        }
    }
    let index = imports
        .far_return
        .expect("far return descriptor helper")
        .emit_binding(code);
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_const(i32::from(width == FarWidth::Word))
        .call(index)
        .local_set(HELPER_STATUS);
    descriptor_result(code, exit_depth);
    cpu_result::validate(code, 0, 0, false, exit_depth);
    target_echo(code, exit_depth);
    code.local_get(esp)
        .i32_const((2 * slot + u32::from(stack_adjust)) as i32)
        .i32_add()
        .local_set(esp)
        .local_get(RESULT)
        .local_set(EIP);
}
