use super::{CTX, FLAGS, KIND, Output, WRITE_STATUS, mem};
use crate::cpu::{
    core::StopReason,
    x86::{ControlOperation, ir::Operation},
};
use wasm_encoder::BlockType;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::X87 { .. }
            | Operation::Simd(_)
            | Operation::Avx(_)
            | Operation::XState(_)
            | Operation::InitializeX87
            | Operation::ClearX87Exceptions
            | Operation::X87StatusToAx
            | Operation::X87StatusToMemory { .. }
            | Operation::X87ControlToMemory { .. }
            | Operation::CpuId
            | Operation::VirtualTimestamp
            | Operation::VirtualTimestampAux
            | Operation::ReadProcessorId { .. }
            | Operation::SoftwareInterrupt { .. }
            | Operation::Control(ControlOperation::Sysenter)
    )
}

pub(super) fn emit(out: &mut Output<'_>, next: u32) {
    let ordinal = out
        .native
        .iter()
        .position(|instruction| instruction.pc == out.pc)
        .expect("native instruction is retained");
    out.materialize(super::super::flags::ARITHMETIC);
    out.c().i32_const(0).local_set(KIND);
    out.publish_operands();
    out.c()
        .local_get(CTX)
        .i32_const(ordinal as i32)
        .call(2)
        .local_set(WRITE_STATUS);
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(9)
        .i32_gt_u()
        .if_(BlockType::Empty);
    out.exit(StopReason::Infrastructure, Some(out.pc), out.prefix);
    out.c().end();
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::CodeInvalidated as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    reload(out);
    out.exit(StopReason::CodeInvalidated, Some(next), out.prefix + 1);
    out.c().end();
    out.c().local_get(WRITE_STATUS).if_(BlockType::Empty);
    out.publish_exit(Some(out.pc), out.prefix);
    out.c().local_get(WRITE_STATUS).return_().end();
    reload(out);
    out.set_pc(next);
}

fn reload(out: &mut Output<'_>) {
    for register in 0..8 {
        out.c()
            .local_get(CTX)
            .i32_load(mem(u64::from(register * 4), 2))
            .local_set(register + 2);
    }
    out.c()
        .local_get(CTX)
        .i32_load(mem(36, 2))
        .local_set(FLAGS)
        .i32_const(0)
        .local_set(KIND);
}
