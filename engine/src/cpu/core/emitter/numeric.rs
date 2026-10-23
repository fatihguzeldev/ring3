mod direct;
mod double;
mod frame;
mod ieee;
mod memory;
mod simd;
pub(super) mod span;

pub(super) const BANK_POINTER: u64 = 92;
pub(super) const SIMD_MXCSR: u64 = 112;
pub(super) const SIMD_XMM: u64 = 116;

use super::{CTX, FLAGS, KIND, Output, WRITE_STATUS, mem};
use crate::cpu::{
    core::StopReason,
    x86::{ControlOperation, X87Condition, X87Operation, ir::Operation},
};
use wasm_encoder::BlockType;

pub(super) use frame::{emit_edge, emit_frame, emit_normalizer, enter_block, normalize_frame};
pub(super) use frame::{
    emit_normalizer_adapter, emit_normalizer_kernel, normalizer_kernel_parameters,
    normalizer_kernel_results, normalizer_parameters, normalizer_results,
};

pub(super) fn enter_port(out: &mut Output<'_>) {
    // An interior port imports the actual identity state, never the skipped
    // prefix's aliases, stack shape, or historical metadata.
    span::invalidate(out);
    out.c()
        .i32_const(1)
        .local_set(frame::EXACT_SUFFIX)
        .i32_const(0)
        .local_set(frame::SELECTED);
}

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::StackFrame { .. }
            | Operation::X87 { .. }
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

pub(super) fn preserves_fp_success(operation: &Operation) -> bool {
    simd::preserves_fp_success(operation)
}

#[derive(Clone, Copy, Default)]
struct X87Transfer {
    inputs: u8,
    outputs: u8,
    flags: u32,
    flags_output: bool,
}

impl X87Transfer {
    fn for_operation(operation: Operation) -> Option<Self> {
        let (operation, address) = match operation {
            Operation::X87 {
                operation, address, ..
            } => (operation, address),
            Operation::InitializeX87 => (X87Operation::Initialize, None),
            Operation::ClearX87Exceptions => (X87Operation::ClearExceptions, None),
            Operation::X87StatusToAx => (X87Operation::StatusToAx, None),
            Operation::X87StatusToMemory { address } => (X87Operation::StoreStatus, Some(address)),
            Operation::X87ControlToMemory { address } => {
                (X87Operation::StoreControl, Some(address))
            }
            _ => return None,
        };
        let mut transfer = Self::default();
        if operation.read_size() != 0 || operation.write_size() != 0 {
            if let Some(address) = address {
                for register in address.base.into_iter().chain(address.index) {
                    transfer.inputs |= 1 << register.index();
                }
            }
        }
        if operation == X87Operation::StatusToAx {
            transfer.inputs |= 1;
            transfer.outputs = 1;
        }
        match operation {
            X87Operation::ConditionalMove { condition, .. } => {
                use super::super::flags::{CF, PF, ZF};
                transfer.flags = match condition {
                    X87Condition::Below | X87Condition::NotBelow => CF,
                    X87Condition::Equal | X87Condition::NotEqual => ZF,
                    X87Condition::BelowOrEqual | X87Condition::Above => CF | ZF,
                    X87Condition::Unordered | X87Condition::Ordered => PF,
                };
            }
            X87Operation::Compare {
                integer_flags: true,
                ..
            } => {
                transfer.flags = super::super::flags::ARITHMETIC;
                transfer.flags_output = true;
            }
            _ => {}
        }
        Some(transfer)
    }
}

pub(super) fn emit(out: &mut Output<'_>, next: u32) {
    let operation = out
        .native
        .iter()
        .find(|instruction| instruction.pc == out.pc)
        .unwrap()
        .operation;
    if direct::state(out, operation) {
        slow(out, next, true);
        out.c().end();
        return;
    }
    out.c().local_get(frame::EXACT_SUFFIX).if_(BlockType::Empty);
    slow(out, next, false);
    out.c().else_();
    let direct = direct::begin(out, operation, next) || simd::begin(out, operation, next);
    slow(out, next, true);
    if direct {
        out.c().end();
    }
    out.c().end();
}

fn slow(out: &mut Output<'_>, next: u32, planned: bool) {
    out.address_fact = None;
    if planned && let Some(before) = out.before_frame {
        frame::emit_frame(out, before);
        frame::normalize_frame(out);
    }
    span::invalidate(out);
    out.c().i32_const(1).local_set(frame::EXACT_SUFFIX);
    let ordinal = out
        .native
        .iter()
        .position(|instruction| instruction.pc == out.pc)
        .expect("native instruction is retained");
    let transfer = X87Transfer::for_operation(out.native[ordinal].operation);
    if let Some(transfer) = transfer {
        if transfer.flags != 0 {
            out.materialize(transfer.flags);
            if transfer.flags_output {
                out.c().i32_const(0).local_set(KIND);
            }
            out.c()
                .local_get(CTX)
                .local_get(FLAGS)
                .i32_store(mem(36, 2));
        }
        for register in 0..8 {
            if transfer.inputs & (1 << register) != 0 {
                out.c()
                    .local_get(CTX)
                    .local_get(register + 2)
                    .i32_store(mem(u64::from(register * 4), 2));
            }
        }
    } else {
        out.materialize(super::super::flags::ARITHMETIC);
        out.c().i32_const(0).local_set(KIND);
        out.publish_operands();
    }
    crate::cpu::core::memory::authority::flush(out.c().raw());
    out.c()
        .local_get(CTX)
        .i32_const(ordinal as i32)
        .call(2)
        .local_set(WRITE_STATUS);
    crate::cpu::core::memory::authority::load(out.c().raw());
    crate::cpu::core::memory::cache::invalidate(out.c().raw());
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::SystemEntry as i32)
        .i32_gt_u()
        .if_(BlockType::Empty);
    out.exit(StopReason::Infrastructure, Some(out.pc), out.prefix);
    out.c().end();
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::CodeInvalidated as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    reload(out, transfer);
    out.exit(StopReason::CodeInvalidated, Some(next), out.prefix + 1);
    out.c().end();
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(StopReason::SystemEntry as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    reload(out, transfer);
    out.exit(StopReason::SystemEntry, Some(next), out.prefix + 1);
    out.c().end();
    out.c().local_get(WRITE_STATUS).if_(BlockType::Empty);
    out.publish_exit(Some(out.pc), out.prefix);
    out.c().local_get(WRITE_STATUS).return_().end();
    reload(out, transfer);
}

fn reload(out: &mut Output<'_>, transfer: Option<X87Transfer>) {
    let outputs = transfer.map_or(u8::MAX, |transfer| transfer.outputs);
    for register in 0..8 {
        if outputs & (1 << register) != 0 {
            out.c()
                .local_get(CTX)
                .i32_load(mem(u64::from(register * 4), 2))
                .local_set(register + 2);
        }
    }
    if transfer.is_none_or(|transfer| transfer.flags_output) {
        out.c()
            .local_get(CTX)
            .i32_load(mem(36, 2))
            .local_set(FLAGS)
            .i32_const(0)
            .local_set(KIND);
    }
}
