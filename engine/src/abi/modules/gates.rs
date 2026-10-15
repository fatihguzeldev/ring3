use super::{AbiError, header};
use crate::cpu::dbt::GateSpec;

pub const MODULE_GATES_HEADER_SIZE: usize = 24;
pub const MODULE_GATE_PAGE_HEADER_SIZE: usize = 32;

pub fn module_gates_size(count: usize) -> Result<usize, AbiError> {
    count
        .checked_mul(8)
        .and_then(|size| size.checked_add(MODULE_GATES_HEADER_SIZE))
        .filter(|size| *size <= u32::MAX as usize)
        .ok_or(AbiError::Length)
}

pub fn encode_module_gates(gates: &[GateSpec], output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != module_gates_size(gates.len())? {
        return Err(AbiError::Length);
    }
    if gates.iter().any(|gate| gate.id == 0)
        || gates
            .windows(2)
            .any(|pair| pair[0].entry.0 >= pair[1].entry.0)
    {
        return Err(AbiError::Module);
    }
    header::write(output, *b"R3MG");
    header::write_u32(output, 16, gates.len() as u32);
    for (index, gate) in gates.iter().enumerate() {
        header::write_u32(output, 24 + index * 8, gate.entry.0);
        header::write_u32(output, 28 + index * 8, gate.id);
    }
    Ok(())
}

pub fn encode_module_gate_page(
    gates: &[GateSpec],
    total: u32,
    start: u32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    let count = u32::try_from(gates.len()).map_err(|_| AbiError::Length)?;
    let size = gates
        .len()
        .checked_mul(8)
        .and_then(|size| size.checked_add(MODULE_GATE_PAGE_HEADER_SIZE))
        .ok_or(AbiError::Length)?;
    if output.len() != size {
        return Err(AbiError::Length);
    }
    if start.checked_add(count).is_none_or(|end| end > total)
        || gates.iter().any(|gate| gate.id == 0)
        || gates
            .windows(2)
            .any(|pair| pair[0].entry.0 >= pair[1].entry.0)
    {
        return Err(AbiError::Module);
    }
    header::write(output, *b"R3GP");
    header::write_u32(output, 16, total);
    header::write_u32(output, 20, start);
    header::write_u32(output, 24, count);
    for (index, gate) in gates.iter().enumerate() {
        header::write_u32(output, 32 + index * 8, gate.entry.0);
        header::write_u32(output, 36 + index * 8, gate.id);
    }
    Ok(())
}
