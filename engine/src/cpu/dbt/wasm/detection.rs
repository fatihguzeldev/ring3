use wasm_encoder::{BlockType, InstructionSink};

use super::{locals::*, memory::Imports};
use crate::cpu::x86::{ControlOperation, decode::DecodedInstruction, ir::Operation};

#[derive(Clone, Copy)]
pub(super) struct DetectionLocals {
    pub(super) epoch: u32,
    origin: u32,
    pc: u32,
    retired: u32,
    flags: u32,
    shadow: u32,
    work: u32,
    has_work: bool,
}

impl DetectionLocals {
    pub(super) const fn new(first: u32, has_work: bool) -> Self {
        Self {
            epoch: first,
            origin: first + 1,
            pc: first + 2,
            retired: first + 3,
            flags: first + 4,
            shadow: first + 5,
            work: first + 6,
            has_work,
        }
    }
}

pub(super) fn snapshot(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: Imports,
) {
    let Some(locals) = imports.detection else {
        return;
    };
    if !matches!(
        instruction.operation(),
        Operation::Control(
            ControlOperation::Breakpoint
                | ControlOperation::Overflow
                | ControlOperation::DebugTrap
                | ControlOperation::Sysenter
        ) | Operation::SoftwareInterrupt {
            vector: 4 | 0x29 | 0x2e
        }
    ) {
        return;
    }
    code.i32_const(instruction.pc().0 as i32)
        .local_set(locals.pc);
    for (source, target) in [
        (RETIRED, locals.retired),
        (FLAGS, locals.flags),
        (DEBUG_SHADOW, locals.shadow),
    ] {
        code.local_get(source).local_set(target);
    }
    if locals.has_work {
        code.local_get(ORIGINAL_BUDGET)
            .local_get(BUDGET)
            .i32_sub()
            .local_set(locals.work);
    }
}

pub(super) fn mark_debug(
    code: &mut InstructionSink<'_>,
    locals: Option<DetectionLocals>,
    origin: i32,
) {
    if let Some(locals) = locals {
        code.i32_const(origin).local_set(locals.origin);
    }
}

pub(super) fn finish(code: &mut InstructionSink<'_>, imports: Imports) {
    let Some(locals) = imports.detection else {
        return;
    };
    // need-code releases an unpublished helper witness; events seal the entry lease.
    code.local_get(REASON)
        .i32_const(3)
        .i32_eq()
        .local_get(REASON)
        .i32_const(5)
        .i32_eq()
        .i32_or()
        .local_get(REASON)
        .i32_const(10)
        .i32_ge_u()
        .local_get(REASON)
        .i32_const(24)
        .i32_le_u()
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(locals.epoch);
    for local in [REASON, DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.local_get(local);
    }
    code.local_get(REASON)
        .i32_const(16)
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(locals.origin)
        .else_()
        .i32_const(0)
        .end();
    // the five restart fields are zero for faults and completed/partial-REP debug traps.
    code.local_get(REASON)
        .i32_const(17)
        .i32_eq()
        .local_get(REASON)
        .i32_const(18)
        .i32_eq()
        .i32_or()
        .local_get(REASON)
        .i32_const(24)
        .i32_eq()
        .i32_or()
        .local_get(REASON)
        .i32_const(16)
        .i32_eq()
        .local_get(locals.origin)
        .i32_const(1)
        .i32_eq()
        .i32_and()
        .i32_or()
        .local_set(LHS);
    for local in [
        locals.pc,
        locals.retired,
        locals.flags,
        locals.shadow,
        locals.work,
    ] {
        code.local_get(LHS)
            .if_(BlockType::Result(wasm_encoder::ValType::I32));
        if local == locals.shadow {
            code.local_get(local)
                .i32_const(if locals.has_work { 2 } else { 0 })
                .i32_or();
        } else {
            code.local_get(local);
        }
        code.else_().i32_const(0).end();
    }
    code.call(
        imports
            .finish_detection
            .expect("configured detection finish import"),
    )
    .local_tee(LHS)
    .if_(BlockType::Empty)
    .local_get(LHS)
    .return_()
    .end()
    .end();
}
