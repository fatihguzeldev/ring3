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
            AvxSemantic::Compare(CompareInstruction::Flags {
                element: Element::Single,
                first: s.vector(1),
                second: s.source(),
                quiet: true,
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
            AvxSemantic::Compare(CompareInstruction::Flags {
                element: Element::Double,
                first: s.vector(1),
                second: s.source(),
                quiet: true,
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
            AvxSemantic::Compare(CompareInstruction::Flags {
                element: Element::Single,
                first: s.vector(1),
                second: s.source(),
                quiet: false,
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
            AvxSemantic::Compare(CompareInstruction::Flags {
                element: Element::Double,
                first: s.vector(1),
                second: s.source(),
                quiet: false,
            })
        }
        5 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Packed {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                },
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        6 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Packed {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                },
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        7 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Packed {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                },
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        8 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Packed {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                },
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
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
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Scalar(Element::Single),
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
            AvxSemantic::Compare(CompareInstruction::Vector {
                shape: CompareShape::Scalar(Element::Double),
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
