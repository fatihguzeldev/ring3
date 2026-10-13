use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    source: s.source(),
                },
            })
        }
        2 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::SquareRoot,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::ReciprocalSquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::ReciprocalSquareRoot,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::ReciprocalSquareRoot,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Reciprocal,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    source: s.source(),
                },
            })
        }
        11 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Reciprocal,
                destination: s.vector(0),
                form: FloatForm::PackedUnary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    source: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Reciprocal,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        17 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        18 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        19 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        20 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        21 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        22 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        23 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        24 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Multiply,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        25 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        26 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        27 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        28 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        29 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        30 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Subtract,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
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
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        32 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        33 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        34 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        35 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        36 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Minimum,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        37 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        38 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        39 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        40 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        41 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        42 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Divide,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        43 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        44 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        45 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Xmm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        46 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::PackedBinary {
                    width: VectorWidth::Ymm,
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        47 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        48 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Floating(FloatInstruction {
                kind: FloatOperation::Maximum,
                destination: s.vector(0),
                form: FloatForm::Scalar {
                    element: Element::Double,
                    first: s.vector(1),
                    second: s.source(),
                },
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
