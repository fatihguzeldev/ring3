use super::DetectedEvent32;
use crate::{
    abi::x86::USER_FLAGS_MASK_V3,
    cpu::SystemEntryKind32,
    cpu::x86::{DebugCause32, EventOrigin32, ExceptionCause32, FrameInput32},
};

/// trusted pre-instruction locals for an instruction-origin trap, not a decoded next-PC delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct InstructionTrapRestart32 {
    pub(in crate::process) eip: u32,
    pub(in crate::process) retired: u32,
    pub(in crate::process) flags: u32,
    pub(in crate::process) ss_debug_shadow: bool,
    /// present exactly when the generated module charges its shared REP work record.
    pub(in crate::process) work_consumed: Option<u32>,
}

pub(super) fn valid_provenance(
    event: DetectedEvent32,
    restart: Option<InstructionTrapRestart32>,
    input: FrameInput32,
    retired: u32,
    ss_debug_shadow: bool,
    work_consumed: u32,
) -> bool {
    let instruction_trap = matches!(
        event,
        DetectedEvent32::Exception(EventOrigin32::SystemEntry(
            SystemEntryKind32::Interrupt(0x29 | 0x2e) | SystemEntryKind32::Sysenter
        )) | DetectedEvent32::Exception(EventOrigin32::Exception(
            ExceptionCause32::BreakpointInt3
                | ExceptionCause32::OverflowInto
                | ExceptionCause32::Debug(DebugCause32::Int1)
        ))
    );
    match (instruction_trap, restart) {
        (true, Some(restart)) => {
            restart.flags & 2 != 0
                && restart.flags & !USER_FLAGS_MASK_V3 == 0
                && input.flags == restart.flags & !0x10000
                && !ss_debug_shadow
                && restart.retired.checked_add(1) == Some(retired)
                && restart
                    .work_consumed
                    .is_none_or(|prior| prior.checked_add(1) == Some(work_consumed))
        }
        (false, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
