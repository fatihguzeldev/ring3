use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: false,
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        2 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: false,
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        3 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: false,
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        4 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: false,
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        5 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: true,
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        6 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: true,
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        7 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: true,
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        8 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Interleave {
                high: true,
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        9 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Shuffle {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        10 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Shuffle {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        11 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Shuffle {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        12 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Shuffle {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        13 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Variable {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.vector(1),
                control: s.source(),
            })
        }
        14 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Variable {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.vector(1),
                control: s.source(),
            })
        }
        15 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Variable {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.vector(1),
                control: s.source(),
            })
        }
        16 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Variable {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.vector(1),
                control: s.source(),
            })
        }
        17 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Immediate {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        18 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Immediate {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        19 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Immediate {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        20 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Immediate {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        21 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Blend {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        22 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Blend {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        23 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Blend {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        24 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::Blend {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        25 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::ExtractSingle {
                source: s.vector(2),
                destination: s.word_target(),
                immediate,
            })
        }
        26 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::InsertSingle {
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        27 => {
            s.validate(
                D | A | B | M,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::BlendVariable {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                mask: s.vector(3),
            })
        }
        28 => {
            s.validate(
                D | A | B | M,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::BlendVariable {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                mask: s.vector(3),
            })
        }
        29 => {
            s.validate(
                D | A | B | M,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::BlendVariable {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                mask: s.vector(3),
            })
        }
        30 => {
            s.validate(
                D | A | B | M,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::BlendVariable {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                mask: s.vector(3),
            })
        }
        31 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::WholeDwords {
                destination: s.vector(0),
                indices: s.vector(1),
                source: s.source(),
            })
        }
        32 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Permute(PermuteInstruction::WholeQwords {
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}

#[cfg(test)]
#[path = "permute/whole/native.rs"]
mod whole;
