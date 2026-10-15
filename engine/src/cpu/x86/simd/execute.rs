use super::{
    BitTestKind, PackedKind, ShuffleKind, SimdError, SimdKind, SimdOperand, SimdOperation,
    SimdRegister, SimdResult, SimdState, crypto, descriptor::validate_operation, float, packed,
    predicates,
};

fn read_operand(
    op: SimdOperand,
    state: &SimdState,
    mmx: &[[u8; 8]; 8],
    gpr: u32,
    memory: &[u8; 16],
) -> [u8; 16] {
    let mut value = [0; 16];
    match op {
        SimdOperand::Register(SimdRegister::Xmm(index)) => value = state.xmm[index as usize],
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            value[..8].copy_from_slice(&mmx[index as usize])
        }
        SimdOperand::Register(SimdRegister::Gpr(_)) => {
            value[..4].copy_from_slice(&gpr.to_le_bytes())
        }
        SimdOperand::Memory(_) => value = *memory,
        SimdOperand::None => {}
    }
    value
}

pub fn execute(
    op: SimdOperation,
    state: &mut SimdState,
    mmx: &mut [[u8; 8]; 8],
    memory: &mut [u8; 16],
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    validate_operation(op)?;
    execute_validated(
        op,
        state,
        mmx,
        memory,
        source_gpr,
        destination_gpr,
        incoming_flags,
    )
}

pub(crate) fn execute_validated(
    op: SimdOperation,
    state: &mut SimdState,
    mmx: &mut [[u8; 8]; 8],
    memory: &mut [u8; 16],
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    use SimdKind as K;
    if state.mxcsr & !0xffff != 0 {
        return Err(SimdError::InvalidState);
    }
    if matches!(op.kind, K::SaveExtended | K::RestoreExtended) {
        return Err(SimdError::InvalidDescriptor);
    }
    if matches!(op.kind, K::EmptyMmx | K::Fence | K::Prefetch) {
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    if op.kind == K::LoadMxcsr {
        let value = u32::from_le_bytes(memory[..4].try_into().unwrap());
        if value & !0xffff != 0 {
            return Err(SimdError::GeneralProtection(0));
        }
        state.mxcsr = value;
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    if op.kind == K::StoreMxcsr {
        memory[..4].copy_from_slice(&state.mxcsr.to_le_bytes());
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    let a = read_operand(op.destination, state, mmx, destination_gpr, memory);
    let b = read_operand(op.source, state, mmx, source_gpr, memory);
    let size = usize::from(op.vector_bytes);
    let width = usize::from(op.lane_bytes);
    let mut result = SimdResult {
        gpr: destination_gpr,
        flags: incoming_flags,
    };
    if op.kind == K::Packed(PackedKind::Test) {
        result.flags = predicates::flags(&a, &b, BitTestKind::Bits, incoming_flags);
        return Ok(result);
    }
    let mut exception_flags = 0;
    let out = match op.kind {
        K::Move => {
            let mut out = [0; 16];
            let length = width.min(size);
            out[..length].copy_from_slice(&b[..length]);
            out
        }
        K::MoveScalar => {
            let mut out = if matches!(op.source, SimdOperand::Memory(_)) {
                [0; 16]
            } else {
                a
            };
            out[..width].copy_from_slice(&b[..width]);
            out
        }
        K::MoveLow | K::MoveHigh => {
            let mut out = a;
            let high = op.kind == K::MoveHigh;
            if op.writes_memory() {
                out[..8].copy_from_slice(&b[if high { 8 } else { 0 }..if high { 16 } else { 8 }]);
            } else {
                let start = if high { 8 } else { 0 };
                out[start..start + 8].copy_from_slice(&b[..8]);
            }
            out
        }
        K::MaskedStore => {
            result.gpr = (0..size).fold(0, |mask, index| mask | u32::from(b[index] >> 7) << index);
            a
        }
        K::Packed(kind) => packed::packed(kind, a, b, size, width, op.immediate),
        K::Crypto(kind) => crypto::execute(kind, a, b, op.immediate),
        K::Shuffle(kind) => {
            let mut immediate = op.immediate.unwrap_or(0);
            if kind == ShuffleKind::InsertSingle && op.reads_memory() {
                immediate &= 0x3f;
            }
            let (out, gpr) = packed::shuffle(kind, a, b, size, width, immediate, state.xmm[0]);
            if let Some(gpr) = gpr
                && matches!(op.destination, SimdOperand::Register(SimdRegister::Gpr(_)))
            {
                result.gpr = gpr;
            }
            out
        }
        K::Float(_) => {
            let (out, exceptions, flags) = float::floating(op, a, b, state.mxcsr, incoming_flags);
            exception_flags = exceptions;
            result.flags = flags;
            out
        }
        K::Convert { .. } => {
            let (out, exceptions, gpr) = float::convert(op, a, b, state.mxcsr);
            exception_flags = exceptions;
            if let Some(gpr) = gpr {
                result.gpr = gpr;
            }
            out
        }
        K::LoadMxcsr
        | K::StoreMxcsr
        | K::EmptyMmx
        | K::SaveExtended
        | K::RestoreExtended
        | K::Fence
        | K::Prefetch => unreachable!(),
    };
    if exception_flags != 0 {
        state.mxcsr |= exception_flags;
        if exception_flags & !(state.mxcsr >> 7) & 63 != 0 {
            return Err(SimdError::FloatingPoint(state.mxcsr));
        }
    }
    match op.destination {
        _ if op.kind == K::MaskedStore => {}
        SimdOperand::Register(SimdRegister::Xmm(index)) => state.xmm[index as usize] = out,
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            mmx[index as usize].copy_from_slice(&out[..8])
        }
        SimdOperand::Register(SimdRegister::Gpr(_)) => {
            if matches!(op.kind, K::Move) {
                result.gpr = packed::unsigned(&out, 0, 4) as u32;
            }
        }
        SimdOperand::Memory(_) => memory[..usize::from(op.memory_bytes)]
            .copy_from_slice(&out[..usize::from(op.memory_bytes)]),
        SimdOperand::None => {}
    }
    if op.kind == K::MaskedStore {
        *memory = out;
    }
    Ok(result)
}
