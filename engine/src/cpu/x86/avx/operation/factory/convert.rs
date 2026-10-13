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
            AvxSemantic::Convert(ConvertInstruction::FromInteger {
                element: Element::Single,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.word_source(),
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
            AvxSemantic::Convert(ConvertInstruction::FromInteger {
                element: Element::Double,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.word_source(),
            })
        }
        3 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::ToInteger {
                element: Element::Single,
                truncate: true,
                destination: s.general(0),
                source: s.source(),
            })
        }
        4 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::ToInteger {
                element: Element::Double,
                truncate: true,
                destination: s.general(0),
                source: s.source(),
            })
        }
        5 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::ToInteger {
                element: Element::Single,
                truncate: false,
                destination: s.general(0),
                source: s.source(),
            })
        }
        6 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::ToInteger {
                element: Element::Double,
                truncate: false,
                destination: s.general(0),
                source: s.source(),
            })
        }
        7 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToDouble,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        8 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToDouble,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        9 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSingle,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        10 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSingle,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
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
            AvxSemantic::Convert(ConvertInstruction::Scalar {
                source_element: Element::Single,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.source(),
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
            AvxSemantic::Convert(ConvertInstruction::Scalar {
                source_element: Element::Double,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.source(),
            })
        }
        13 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SignedToSingle,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        14 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SignedToSingle,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        15 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToSigned { truncate: false },
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        16 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToSigned { truncate: false },
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        17 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToSigned { truncate: true },
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        18 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SingleToSigned { truncate: true },
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        19 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSigned { truncate: true },
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        20 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSigned { truncate: true },
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        21 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SignedToDouble,
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        22 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::SignedToDouble,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        23 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSigned { truncate: false },
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        24 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Convert(ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSigned { truncate: false },
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
