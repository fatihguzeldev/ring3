use crate::cpu::x86::simd::{
    SimdError, SimdKind, SimdOperand, SimdOperation, SimdRegister, SimdResult, decode_descriptor,
    execute_validated, restore_extended, save_extended,
};

pub(super) fn execute_descriptor(
    arena: &mut [u8],
    descriptor: u32,
    operands: u32,
    immediate: u32,
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    use crate::abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{
            SIMD_SIZE, X87_REGISTERS_OFFSET, X87_STATUS_OFFSET, X87_TAG_OFFSET, decode_simd,
            encode_simd,
        },
    };
    let op = decode_descriptor(descriptor, operands, immediate)?;
    if matches!(op.kind, SimdKind::SaveExtended | SimdKind::RestoreExtended) {
        return extended_descriptor(arena, op, incoming_flags, destination_gpr);
    }
    let mut state = decode_simd(
        arena
            .get(SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE)
            .ok_or(SimdError::InvalidState)?,
    )
    .map_err(|_| SimdError::InvalidState)?;
    if op.uses_mmx() || op.kind == SimdKind::EmptyMmx {
        let x87 =
            crate::abi::x86::decode_x87(&arena[X87_OFFSET..X87_OFFSET + crate::abi::x86::X87_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
        if x87.status & !x87.control & 63 != 0 {
            let status = x87.status | 0x8080;
            arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                .copy_from_slice(&status.to_le_bytes());
            return Err(SimdError::X87FloatingPoint(status));
        }
    }
    let mut mmx = std::array::from_fn(|index| {
        arena[X87_OFFSET + X87_REGISTERS_OFFSET + index * 10
            ..X87_OFFSET + X87_REGISTERS_OFFSET + index * 10 + 8]
            .try_into()
            .unwrap()
    });
    let mut memory: [u8; 16] = arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16]
        .try_into()
        .unwrap();
    let result = execute_validated(
        op,
        &mut state,
        &mut mmx,
        &mut memory,
        source_gpr,
        destination_gpr,
        incoming_flags,
    );
    match result {
        Err(SimdError::FloatingPoint(_)) => {
            encode_simd(&state, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
        }
        Ok(_) if !op.writes_memory() => {
            encode_simd(&state, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
            if op.uses_mmx() {
                if let SimdOperand::Register(SimdRegister::Mmx(index)) = op.destination {
                    let start = X87_OFFSET + X87_REGISTERS_OFFSET + usize::from(index) * 10;
                    arena[start..start + 8].copy_from_slice(&mmx[index as usize]);
                    arena[start + 8..start + 10].copy_from_slice(&[255; 2]);
                }
                arena[X87_OFFSET + X87_TAG_OFFSET..X87_OFFSET + X87_TAG_OFFSET + 2].fill(0);
                let sw = u16::from_le_bytes(
                    arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                        .try_into()
                        .unwrap(),
                ) & !0x3800;
                arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                    .copy_from_slice(&sw.to_le_bytes());
            }
            if op.kind == SimdKind::EmptyMmx {
                arena[X87_OFFSET + X87_TAG_OFFSET..X87_OFFSET + X87_TAG_OFFSET + 2].fill(255);
            }
        }
        Ok(_) => arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].copy_from_slice(&memory),
        Err(_) => {}
    }
    result
}

fn extended_descriptor(
    arena: &mut [u8],
    op: SimdOperation,
    flags: u32,
    gpr: u32,
) -> Result<SimdResult, SimdError> {
    use crate::abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_SIZE, X87_SIZE, decode_simd, decode_x87, encode_simd, encode_x87},
    };
    if arena.len() < SIMD_OFFSET + SIMD_SIZE {
        return Err(SimdError::InvalidState);
    }
    if op.kind == SimdKind::SaveExtended {
        let x87 = decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        let simd = decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        let output: &mut [u8; 512] = (&mut arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 512])
            .try_into()
            .unwrap();
        save_extended(&x87, &simd, output);
    } else {
        let (x87, simd) = restore_extended(
            (&arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 512])
                .try_into()
                .unwrap(),
        )?;
        encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
    }
    Ok(SimdResult { gpr, flags })
}

#[cfg(test)]
mod tests;
