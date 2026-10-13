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
            AvxSemantic::CrossLane(CrossLaneInstruction::Broadcast {
                element: BroadcastElement::Dword,
                source: MoveSource::Memory,
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
            AvxSemantic::CrossLane(CrossLaneInstruction::Broadcast {
                element: BroadcastElement::Dword,
                source: MoveSource::Memory,
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
            AvxSemantic::CrossLane(CrossLaneInstruction::Broadcast {
                element: BroadcastElement::Qword,
                width: VectorWidth::Ymm,
                source: MoveSource::Memory,
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
        8..=15 => {
            use BroadcastElement::*;
            use VectorWidth::*;
            let (element, width, source) = match kind {
                8 => (Byte, Xmm, MemoryPolicy::Optional),
                9 => (Byte, Ymm, MemoryPolicy::Optional),
                10 => (Word, Xmm, MemoryPolicy::Optional),
                11 => (Word, Ymm, MemoryPolicy::Optional),
                12 => (Qword, Xmm, MemoryPolicy::Optional),
                13 => (Dword, Xmm, MemoryPolicy::Forbidden),
                14 => (Dword, Ymm, MemoryPolicy::Forbidden),
                15 => (Qword, Ymm, MemoryPolicy::Forbidden),
                _ => unreachable!("closed scalar broadcast templates"),
            };
            s.validate(D | B, source, MemoryPolicy::Forbidden, false, immediate)?;
            AvxSemantic::CrossLane(CrossLaneInstruction::Broadcast {
                element,
                width,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}

#[cfg(test)]
#[path = "cross_lane/broadcast/native.rs"]
mod broadcast;
