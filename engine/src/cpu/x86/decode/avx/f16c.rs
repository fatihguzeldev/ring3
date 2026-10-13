use super::*;

pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let (kind, length, narrow) = match instruction.code() {
        Code::VEX_Vcvtph2ps_xmm_xmmm64 => (1, 0, false),
        Code::VEX_Vcvtph2ps_ymm_xmmm128 => (2, 1, false),
        Code::VEX_Vcvtps2ph_xmmm64_xmm_imm8 => (3, 0, true),
        Code::VEX_Vcvtps2ph_xmmm128_ymm_imm8 => (4, 1, true),
        _ => return None,
    };
    Some((|| {
        encoding.validate(true, length, true)?;
        let mut operands = index(instruction, 0, false, narrow)?;
        operands |= index(instruction, 1, false, !narrow)? << 6;
        if instruction.op_kind(0) == OpKind::Memory {
            operands |= 1 << 13;
        }
        if instruction.op_kind(1) == OpKind::Memory {
            operands |= 1 << 12;
        }
        let immediate = if narrow {
            u32::from(instruction.immediate8())
        } else {
            0
        };
        let typed = AvxInstruction::decode_descriptor(0x01001200 | kind, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, typed)
    })())
}

#[cfg(test)]
mod tests;
