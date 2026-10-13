use super::*;
use crate::cpu::x86::{
    Register32, SegmentAccess, SegmentRegister, USER_IRET_FLAGS_MASK, USER_IRET_FLAGS_WORD_MASK,
};

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct InterruptReturnLocals {
    target: u32,
    selector: u32,
    flags: u32,
}

impl InterruptReturnLocals {
    pub(in crate::cpu::dbt::wasm) fn new(first: u32) -> Self {
        Self {
            target: first,
            selector: first + 1,
            flags: first + 2,
        }
    }
}

pub(in crate::cpu::dbt::wasm) fn interrupt_return(
    code: &mut InstructionSink<'_>,
    width: FarWidth,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let saved = imports
        .interrupt_return_locals
        .expect("iret capture locals");
    let slot = width.slot_bytes();
    let esp = register(Register32::Esp);
    // B32 permits wrapping between slots; each whole slot keeps its cached authority.
    for offset in [0, slot, 2 * slot] {
        code.local_get(esp)
            .i32_const(offset as i32)
            .i32_add()
            .local_set(ADDRESS);
        super::super::segments::check_span(
            code,
            SegmentRegister::Ss,
            slot,
            SegmentAccess::Read,
            imports,
            exit_depth,
        );
    }
    for (offset, destination) in [
        (0, saved.target),
        (slot, saved.selector),
        (2 * slot, saved.flags),
    ] {
        code.local_get(esp).i32_const(offset as i32).i32_add();
        super::super::segments::base(code, SegmentRegister::Ss);
        code.i32_add()
            .local_tee(ADDRESS)
            .i32_const(slot as i32)
            .i32_const(1)
            .call(imports.cpu_read.expect("iret stack read"))
            .local_set(HELPER_STATUS);
        crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
        cpu_result::validate(code, slot, 1, false, exit_depth);
        code.local_get(STATE_PTR);
        if destination == saved.selector || width == FarWidth::Word {
            code.i32_load16_u(transfer(0));
        } else {
            code.i32_load(transfer(0));
        }
        code.local_set(destination);
    }
    code.local_get(saved.target)
        .local_set(RESULT)
        .local_get(saved.selector)
        .local_set(ADDRESS);
    let helper = imports
        .far_return
        .expect("iret descriptor return helper")
        .emit_binding(code);
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_const(i32::from(width == FarWidth::Word))
        .call(helper)
        .local_set(HELPER_STATUS);
    descriptor_result(code, exit_depth);
    cpu_result::validate(code, 0, 0, false, exit_depth);
    target_echo(code, exit_depth);
    if let Some(limit) = imports.code_fetch_limit {
        super::super::code_fetch::refresh(code, limit);
    }
    let mutable = if width == FarWidth::Word {
        USER_IRET_FLAGS_WORD_MASK
    } else {
        USER_IRET_FLAGS_MASK
    };
    code.local_get(FLAGS)
        .i32_const(!mutable as i32)
        .i32_and()
        .local_get(saved.flags)
        .i32_const(mutable as i32)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    code.local_get(esp)
        .i32_const((3 * slot) as i32)
        .i32_add()
        .local_set(esp)
        .local_get(saved.target)
        .local_set(EIP);
}
