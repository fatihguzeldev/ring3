mod direct;
mod double;
mod frame;
mod ieee;
mod memory;
pub(super) mod pool;
pub(super) mod record;
mod service;
mod simd;
pub(super) mod span;

pub(super) const BANK_POINTER: u64 = 92;
pub(super) const SIMD_MXCSR: u64 = 112;
pub(super) const SIMD_XMM: u64 = 116;

use super::{Output, mem};
use crate::cpu::x86::{ControlOperation, X87Condition, X87Operation, ir::Operation};
use wasm_encoder::BlockType;

pub(super) use frame::{
    descriptor_parameters, descriptor_results, emit_descriptor, emit_inline_commit,
    emit_normalizer_adapter, emit_normalizer_kernel, normalizer_kernel_parameters,
    normalizer_kernel_results, normalizer_parameters, normalizer_results,
};
pub(super) use frame::{emit_edge, emit_frame, enter_block};
pub(super) use service::emit as emit_service;

pub(super) fn enter_port(out: &mut Output<'_>) {
    // An interior port imports the actual identity state, never the skipped
    // prefix's aliases, stack shape, or historical metadata.
    // A service continuation already relinquished its local view. Pure scalar
    // services can retain the pool; a later canonical callback observes it then.
    // Fresh external/planned interior ports still publish their identity first.
    out.c()
        .local_get(frame::EXACT_SUFFIX)
        .i32_eqz()
        .if_(BlockType::Empty);
    span::invalidate(out);
    out.c().end();
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
        out.exit(super::StopReason::ExactPrefix, Some(out.pc), out.prefix);
        out.c().end();
        return;
    }
    let direct = direct::begin(out, operation, next) || simd::begin(out, operation, next);
    if direct {
        // A failed speculative fast admission has not executed this instruction.
        // Reconstruct the completed prefix and let the exact executor own it.
        out.exit(super::StopReason::ExactPrefix, Some(out.pc), out.prefix);
        out.c().end();
    } else {
        // Planned canonical effects retain the selected module's own ordinal.
        service::request(out, next, true);
    }
}
