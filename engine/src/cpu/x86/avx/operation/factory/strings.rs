use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::String(StringInstruction::from_parts(
                s.vector(1),
                s.source(),
                PackedStringLength::Explicit,
                PackedStringOutput::Mask,
                immediate,
            ))
        }
        2 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::String(StringInstruction::from_parts(
                s.vector(1),
                s.source(),
                PackedStringLength::Explicit,
                PackedStringOutput::Index,
                immediate,
            ))
        }
        3 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::String(StringInstruction::from_parts(
                s.vector(1),
                s.source(),
                PackedStringLength::NullTerminated,
                PackedStringOutput::Mask,
                immediate,
            ))
        }
        4 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::String(StringInstruction::from_parts(
                s.vector(1),
                s.source(),
                PackedStringLength::NullTerminated,
                PackedStringOutput::Index,
                immediate,
            ))
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
