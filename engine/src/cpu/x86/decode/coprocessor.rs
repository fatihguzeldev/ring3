use crate::cpu::x86::{
    ControlOperation, X87Operation, ir::Operation, simd::SimdKind, xstate::XStateOperation,
};
use iced_x86::{Code, Instruction};

/// only admitted semantics reach this boundary; prefix, feature and unsupported owners remain earlier.
pub(super) fn fault(
    instruction: &Instruction,
    operation: &Operation,
    cr0: Option<u32>,
) -> Option<ControlOperation> {
    let cr0 = cr0?;
    let em = cr0 & 4 != 0;
    let ts = cr0 & 8 != 0;
    // these exact obsolete x87 opcodes have no FPU-state effect, but remain x87 instructions.
    if matches!(
        instruction.code(),
        Code::Fneni | Code::Fndisi | Code::Fnsetpm
    ) {
        return (em || ts).then_some(ControlOperation::DeviceNotAvailable);
    }
    let unavailable = match operation {
        Operation::X87 {
            operation: X87Operation::Wait,
            ..
        } => ts && cr0 & 2 != 0,
        Operation::X87 { .. }
        | Operation::InitializeX87
        | Operation::ClearX87Exceptions
        | Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. } => em || ts,
        Operation::Simd(operation) => match operation.kind {
            SimdKind::Fence | SimdKind::Prefetch => false,
            SimdKind::SaveExtended | SimdKind::RestoreExtended => em || ts,
            _ if instruction.code() == Code::Movnti_m32_r32 => false,
            _ => {
                if em {
                    return Some(ControlOperation::InvalidOpcode);
                }
                ts
            }
        },
        Operation::PackedStringCompare(_) => {
            if em {
                return Some(ControlOperation::InvalidOpcode);
            }
            ts
        }
        // vex and xsave-managed transfers have their own ts rule, not legacy sse's em rule.
        Operation::Avx(_)
        | Operation::XState(XStateOperation::Save { .. } | XStateOperation::Restore { .. }) => ts,
        _ => false,
    };
    unavailable.then_some(ControlOperation::DeviceNotAvailable)
}

#[cfg(test)]
mod tests;
