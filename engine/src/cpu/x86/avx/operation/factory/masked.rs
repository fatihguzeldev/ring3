use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                B | M,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::StoreBytes {
                source: s.vector(2),
                mask: s.vector(3),
            })
        }
        2 => {
            s.validate(
                D | M,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Load {
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: s.vector(0),
                mask: s.vector(3),
            })
        }
        3 => {
            s.validate(
                D | M,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Load {
                width: VectorWidth::Ymm,
                element: Element::Single,
                destination: s.vector(0),
                mask: s.vector(3),
            })
        }
        4 => {
            s.validate(
                D | M,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Load {
                width: VectorWidth::Xmm,
                element: Element::Double,
                destination: s.vector(0),
                mask: s.vector(3),
            })
        }
        5 => {
            s.validate(
                D | M,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Load {
                width: VectorWidth::Ymm,
                element: Element::Double,
                destination: s.vector(0),
                mask: s.vector(3),
            })
        }
        6 => {
            s.validate(
                B | M,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Store {
                width: VectorWidth::Xmm,
                element: Element::Single,
                source: s.vector(2),
                mask: s.vector(3),
            })
        }
        7 => {
            s.validate(
                B | M,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Store {
                width: VectorWidth::Ymm,
                element: Element::Single,
                source: s.vector(2),
                mask: s.vector(3),
            })
        }
        8 => {
            s.validate(
                B | M,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Store {
                width: VectorWidth::Xmm,
                element: Element::Double,
                source: s.vector(2),
                mask: s.vector(3),
            })
        }
        9 => {
            s.validate(
                B | M,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Masked(MaskedInstruction::Store {
                width: VectorWidth::Ymm,
                element: Element::Double,
                source: s.vector(2),
                mask: s.vector(3),
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
