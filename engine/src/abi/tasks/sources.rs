use crate::{
    abi::{AbiError, header},
    cpu::x86::{SegmentDescriptorRecord, tasks::TaskRegister},
};

pub const TASK_RETURN_SOURCES_SIZE: usize = 32;

pub fn decode_task_return_sources(bytes: &[u8]) -> Result<(TaskRegister, u16), AbiError> {
    header::validate(bytes, *b"R3TR", TASK_RETURN_SOURCES_SIZE)?;
    let selector = u16::try_from(header::read_u32(bytes, 16)).map_err(|_| AbiError::TaskSources)?;
    let limit = u16::try_from(header::read_u32(bytes, 20)).map_err(|_| AbiError::TaskSources)?;
    let descriptor = SegmentDescriptorRecord::from_words(
        header::read_u32(bytes, 24),
        header::read_u32(bytes, 28),
    );
    let current =
        TaskRegister::from_cached(selector, descriptor).map_err(|_| AbiError::TaskSources)?;
    Ok((current, limit))
}

pub fn encode_task_return_sources(
    current: TaskRegister,
    gdt_limit: u16,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != TASK_RETURN_SOURCES_SIZE {
        return Err(AbiError::Length);
    }
    let (low, high) = current.descriptor().to_words();
    header::write(output, *b"R3TR");
    for (offset, value) in [
        (16, u32::from(current.selector())),
        (20, u32::from(gdt_limit)),
        (24, low),
        (28, high),
    ] {
        header::write_u32(output, offset, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
