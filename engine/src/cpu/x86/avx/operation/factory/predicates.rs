use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::SingleSign,
                width: VectorWidth::Xmm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        2 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::SingleSign,
                width: VectorWidth::Ymm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        3 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::DoubleSign,
                width: VectorWidth::Xmm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        4 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::DoubleSign,
                width: VectorWidth::Ymm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        5 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::Bits,
                width: VectorWidth::Xmm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        6 => {
            s.validate(
                A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Test(TestInstruction {
                kind: BitTestKind::Bits,
                width: VectorWidth::Ymm,
                first: s.vector(1),
                second: s.source(),
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
