use crate::{
    abi::{AbiError, header, memory_helper::HELPER_SIZE},
    cpu::x86::tasks::{TaskAuthorityRequirement32, TaskTransitionFault32},
    memory::{Access, FaultReason, MemoryError, MemoryFault},
};

pub const TASK_TRANSITION_HELPER_VERSION: u16 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskTransitionReply32 {
    Committed {
        fault: Option<TaskTransitionFault32>,
        code_invalidated: bool,
        task_trap: bool,
    },
    Fault(TaskTransitionFault32),
    Authority(TaskAuthorityRequirement32),
    Memory {
        fault: MemoryFault,
        length: u32,
    },
    Infrastructure(MemoryError),
}

pub fn encode_task_transition_result(
    reply: TaskTransitionReply32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = match reply {
        TaskTransitionReply32::Committed {
            fault,
            code_invalidated,
            task_trap,
        } => {
            let (reason, detail) = fault.map(fault_fields).transpose()?.unwrap_or((0, 0));
            [
                0,
                reason,
                detail,
                0,
                u32::from(code_invalidated) | (u32::from(task_trap) << 1),
                0,
            ]
        }
        TaskTransitionReply32::Fault(fault) => {
            let (reason, detail) = fault_fields(fault)?;
            if !matches!(reason, 14 | 21) || reason == 14 && detail & 4 != 0 {
                return Err(AbiError::MemoryHelper);
            }
            [4, reason, detail, 0, 0, 0]
        }
        TaskTransitionReply32::Authority(requirement) => {
            use TaskAuthorityRequirement32::*;
            let (tag, payload) = match requirement {
                Sources => (1, 0),
                Controls => (2, 0),
                TaskFormat => (3, 0),
                AddressSpace => (4, 0),
                Ldt(selector) => (5, u32::from(selector)),
                Descriptor(selector) => (6, u32::from(selector)),
                Privilege(cpl) if cpl <= 3 => (7, u32::from(cpl)),
                Virtual8086 => (8, 0),
                Flags(flags) => (9, flags),
                SegmentMode(selector) => (10, u32::from(selector)),
                Privilege(_) => return Err(AbiError::MemoryHelper),
            };
            [8, tag, payload, 0, 0, 0]
        }
        TaskTransitionReply32::Memory { fault, length } => {
            if !matches!(length, 2 | 44 | 104) || fault.access == Access::Write && length != 2 {
                return Err(AbiError::MemoryHelper);
            }
            let reason = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => return Err(AbiError::MemoryHelper),
            };
            let access = match fault.access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => return Err(AbiError::MemoryHelper),
            };
            [1, 0, reason, fault.address.0, access, length]
        }
        TaskTransitionReply32::Infrastructure(MemoryError::Fault(_)) => {
            return Err(AbiError::MemoryHelper);
        }
        TaskTransitionReply32::Infrastructure(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            0,
        ],
    };
    let mut bytes = [0; HELPER_SIZE];
    header::write_version(&mut bytes, *b"R3MH", TASK_TRANSITION_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(&mut bytes, 16 + 4 * index, field);
    }
    output.copy_from_slice(&bytes);
    Ok(())
}

fn fault_fields(fault: TaskTransitionFault32) -> Result<(u32, u32), AbiError> {
    use TaskTransitionFault32::*;
    let (reason, code) = match fault {
        InvalidTss(code) => (21, code),
        NotPresent(code) => (14, code),
        StackSegment(code) => (15, code),
        GeneralProtection(code) => (11, code),
    };
    if code > 0xfffc
        || code & 3 != 0
        || reason == 14 && code == 0
        || reason == 11 && code != 0
        || reason == 15 && code == 0
    {
        return Err(AbiError::MemoryHelper);
    }
    Ok((reason, code))
}

#[cfg(test)]
mod tests;
