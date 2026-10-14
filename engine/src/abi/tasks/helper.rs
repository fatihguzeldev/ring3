use crate::{
    abi::{AbiError, header, memory_helper::HELPER_SIZE},
    cpu::x86::tasks::TaskReturnAdmission,
    memory::{Access, FaultReason, MemoryError},
};

pub const TASK_RETURN_HELPER_VERSION: u16 = 13;

/// admission has no successful transition packet; the live source owner certifies read-fault geometry.
pub fn encode_task_return_result(
    backlink_base: Option<u32>,
    result: &TaskReturnAdmission,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = match *result {
        TaskReturnAdmission::InvalidTss(code) if code <= 0xfffc && code & 3 == 0 => {
            [3, 0, code, 0, 0, 0]
        }
        TaskReturnAdmission::NotPresent(code) if code != 0 && code <= 0xfff8 && code & 7 == 0 => {
            [6, 0, code, 0, 0, 0]
        }
        TaskReturnAdmission::MissingAuthority => [8, 0, 1, 0, 0, 0],
        TaskReturnAdmission::Admitted(prepared) => {
            [8, 0, 2, u32::from(prepared.target().selector()), 0, 0]
        }
        TaskReturnAdmission::NeedsPagingAuthority { fault, length } => {
            let base = backlink_base.ok_or(AbiError::MemoryHelper)?;
            if length != 2
                || fault.access != Access::Read
                || fault.address.0.wrapping_sub(base) >= 2
            {
                return Err(AbiError::MemoryHelper);
            }
            let reason = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => return Err(AbiError::MemoryHelper),
            };
            [1, 0, reason, fault.address.0, 1, 2]
        }
        TaskReturnAdmission::Infrastructure(MemoryError::Fault(_)) => {
            return Err(AbiError::MemoryHelper);
        }
        TaskReturnAdmission::Infrastructure(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            2,
        ],
        _ => return Err(AbiError::MemoryHelper),
    };
    let mut packet = [0; HELPER_SIZE];
    header::write_version(&mut packet, *b"R3MH", TASK_RETURN_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(&mut packet, 16 + index * 4, field);
    }
    output.copy_from_slice(&packet);
    Ok(())
}

#[cfg(test)]
mod tests;
