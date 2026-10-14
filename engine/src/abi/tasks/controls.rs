use crate::{
    abi::{AbiError, header},
    cpu::x86::tasks::TaskControlState32,
};

pub const TASK_CONTROLS_SIZE: usize = 40;

pub fn decode_task_controls(bytes: &[u8]) -> Result<TaskControlState32, AbiError> {
    header::validate(bytes, *b"R3CR", TASK_CONTROLS_SIZE)?;
    let ldtr = u16::try_from(header::read_u32(bytes, 24)).map_err(|_| AbiError::TaskControls)?;
    let nmi = match header::read_u32(bytes, 32) {
        0 => None,
        1 => Some(false),
        2 => Some(true),
        _ => return Err(AbiError::TaskControls),
    };
    if header::read_u32(bytes, 36) != 0 {
        return Err(AbiError::TaskControls);
    }
    TaskControlState32::new(
        header::read_u32(bytes, 16),
        header::read_u32(bytes, 20),
        ldtr,
        header::read_u32(bytes, 28),
        nmi,
    )
    .map_err(|_| AbiError::TaskControls)
}

pub fn encode_task_controls(state: TaskControlState32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != TASK_CONTROLS_SIZE {
        return Err(AbiError::Length);
    }
    let mut bytes = [0; TASK_CONTROLS_SIZE];
    header::write(&mut bytes, *b"R3CR");
    for (offset, value) in [
        (16, state.cr0()),
        (20, state.cr3()),
        (24, u32::from(state.ldtr())),
        (28, state.dr7()),
        (
            32,
            state
                .nmi_blocked()
                .map_or(0, |blocked| 1 + u32::from(blocked)),
        ),
    ] {
        header::write_u32(&mut bytes, offset, value);
    }
    output.copy_from_slice(&bytes);
    Ok(())
}

#[cfg(test)]
mod tests;
