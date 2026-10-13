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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Xmm,
                element: Element::Double,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Ymm,
                element: Element::Double,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Xmm,
                element: Element::Single,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Ymm,
                element: Element::Single,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalSubtract,
                width: VectorWidth::Xmm,
                element: Element::Double,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalSubtract,
                width: VectorWidth::Ymm,
                element: Element::Double,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalSubtract,
                width: VectorWidth::Xmm,
                element: Element::Single,
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
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalSubtract,
                width: VectorWidth::Ymm,
                element: Element::Single,
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
                false,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Xmm,
                element: Element::Double,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        10 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Ymm,
                element: Element::Double,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        11 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        12 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Ymm,
                element: Element::Single,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        13 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundPacked {
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        14 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundPacked {
                width: VectorWidth::Ymm,
                element: Element::Single,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        15 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundPacked {
                width: VectorWidth::Xmm,
                element: Element::Double,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        16 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundPacked {
                width: VectorWidth::Ymm,
                element: Element::Double,
                destination: s.vector(0),
                source: s.source(),
                immediate,
            })
        }
        17 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundScalar {
                element: Element::Single,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.source(),
                immediate,
            })
        }
        18 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::RoundScalar {
                element: Element::Double,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.source(),
                immediate,
            })
        }
        19 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::DotSingle {
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        20 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compound(CompoundInstruction::DotSingle {
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
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
            AvxSemantic::Compound(CompoundInstruction::DotDouble {
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
