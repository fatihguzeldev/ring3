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
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::InverseMixColumns,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        2 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::EncryptRound,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        3 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::EncryptLast,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        4 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::DecryptRound,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        5 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::DecryptLast,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        6 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::CarrylessMultiply,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    Some(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        7 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Crypto(
                CryptoInstruction::new(
                    CryptoKind::KeyAssist,
                    s.vector(0),
                    None,
                    s.source(),
                    Some(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
