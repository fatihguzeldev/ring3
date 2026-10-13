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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::And,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::And,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::AndNot,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::AndNot,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::Or,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::Or,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::Xor,
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
            AvxSemantic::Logical(LogicalInstruction {
                kind: LogicalOperation::Xor,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
