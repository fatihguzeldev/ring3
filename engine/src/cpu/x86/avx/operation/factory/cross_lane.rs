use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::CrossLane(CrossLaneInstruction::BroadcastSingle {
                width: VectorWidth::Xmm,
                destination: s.vector(0),
            })
        }
        2 => {
            s.validate(
                D,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::CrossLane(CrossLaneInstruction::BroadcastSingle {
                width: VectorWidth::Ymm,
                destination: s.vector(0),
            })
        }
        3 => {
            s.validate(
                D,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::CrossLane(CrossLaneInstruction::BroadcastDouble {
                destination: s.vector(0),
            })
        }
        4 => {
            s.validate(
                D,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::CrossLane(CrossLaneInstruction::BroadcastBlock {
                destination: s.vector(0),
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
            AvxSemantic::CrossLane(CrossLaneInstruction::Permute {
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
            AvxSemantic::CrossLane(CrossLaneInstruction::Insert {
                destination: s.vector(0),
                first: s.vector(1),
                second: s.source(),
                immediate,
            })
        }
        7 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                true,
                immediate,
            )?;
            AvxSemantic::CrossLane(CrossLaneInstruction::Extract {
                source: s.vector(2),
                destination: s.block_target(),
                immediate,
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
