use crate::{
    abi::{AbiError, header},
    cpu::x86::{InterruptDescriptorTable32, SegmentDescriptorRecord},
};

pub const INTERRUPT_DESCRIPTOR_TABLE_SIZE: usize = 2068;

pub fn encode_interrupt_descriptor_table(
    table: &InterruptDescriptorTable32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != INTERRUPT_DESCRIPTOR_TABLE_SIZE {
        return Err(AbiError::Length);
    }
    let mut staged = [0; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
    header::write_version(&mut staged, *b"R3ID", 1);
    staged[16..18].copy_from_slice(&table.limit().to_le_bytes());
    for vector in 0..=u8::MAX {
        let start = 20 + usize::from(vector) * 8;
        staged[start..start + 8].copy_from_slice(&table.record(vector).to_bytes());
    }
    output.copy_from_slice(&staged);
    Ok(())
}

pub fn decode_interrupt_descriptor_table(
    input: &[u8],
) -> Result<InterruptDescriptorTable32, AbiError> {
    header::validate_versions(input, *b"R3ID", INTERRUPT_DESCRIPTOR_TABLE_SIZE, &[1])?;
    if input[18..20] != [0, 0] {
        return Err(AbiError::Reserved);
    }
    let limit = u16::from_le_bytes(input[16..18].try_into().unwrap());
    // allocate only after the entire fixed envelope is valid; raw slots need no validation.
    let records = Box::new(std::array::from_fn(|vector| {
        let start = 20 + vector * 8;
        SegmentDescriptorRecord::from_bytes(input[start..start + 8].try_into().unwrap())
    }));
    Ok(InterruptDescriptorTable32::new(limit, records))
}

#[cfg(test)]
mod tests;
