use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Insert {
                width: ScalarWidth::Word,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.word_source(),
                immediate,
            })
        }
        2 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Extract {
                width: ScalarWidth::Word,
                source: s.vector(2),
                destination: s.word_target(),
                immediate,
            })
        }
        3 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Mask {
                source: s.vector(2),
                destination: s.general(0),
            })
        }
        4 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                true,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Extract {
                width: ScalarWidth::Byte,
                source: s.vector(2),
                destination: s.word_target(),
                immediate,
            })
        }
        5 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                true,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Extract {
                width: ScalarWidth::Word,
                source: s.vector(2),
                destination: s.word_target(),
                immediate,
            })
        }
        6 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                true,
                immediate,
            )?;
            AvxSemantic::Scalar(ScalarInstruction::Extract {
                width: ScalarWidth::Dword,
                source: s.vector(2),
                destination: s.word_target(),
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
            AvxSemantic::Scalar(ScalarInstruction::Insert {
                width: ScalarWidth::Byte,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.word_source(),
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
            AvxSemantic::Scalar(ScalarInstruction::Insert {
                width: ScalarWidth::Dword,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.word_source(),
                immediate,
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
