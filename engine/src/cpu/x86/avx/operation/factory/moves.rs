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
            AvxSemantic::Move(if s.source_memory() {
                MoveInstruction::Load {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    aligned: false,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
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
            AvxSemantic::Move(if s.source_memory() {
                MoveInstruction::Load {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    aligned: false,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        3 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Scalar {
                element: Element::Single,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.vector(2),
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
            AvxSemantic::Move(MoveInstruction::LoadScalar {
                element: Element::Single,
                destination: s.vector(0),
            })
        }
        5 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Scalar {
                element: Element::Double,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.vector(2),
            })
        }
        6 => {
            s.validate(
                D,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::LoadScalar {
                element: Element::Double,
                destination: s.vector(0),
            })
        }
        7 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(if s.target_memory() {
                MoveInstruction::Store {
                    width: VectorWidth::Xmm,
                    source: s.vector(2),
                    aligned: false,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        8 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(if s.target_memory() {
                MoveInstruction::Store {
                    width: VectorWidth::Ymm,
                    source: s.vector(2),
                    aligned: false,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        9 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::StoreScalar {
                element: Element::Single,
                source: s.vector(2),
            })
        }
        10 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::StoreScalar {
                element: Element::Double,
                source: s.vector(2),
            })
        }
        11 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::MergeHalf {
                kind: HalfMerge::HighToLow,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.vector(2),
            })
        }
        12 => {
            s.validate(
                D | A,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::LoadHalf {
                half: Half::Low,
                destination: s.vector(0),
                merge: s.vector(1),
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::SingleLow,
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::SingleLow,
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::DoubleLow,
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::DoubleLow,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        17 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::StoreHalf {
                half: Half::Low,
                source: s.vector(2),
            })
        }
        18 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::MergeHalf {
                kind: HalfMerge::LowToHigh,
                destination: s.vector(0),
                merge: s.vector(1),
                source: s.vector(2),
            })
        }
        19 => {
            s.validate(
                D | A,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::LoadHalf {
                half: Half::High,
                destination: s.vector(0),
                merge: s.vector(1),
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::SingleHigh,
                width: VectorWidth::Xmm,
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
            AvxSemantic::Move(MoveInstruction::Duplicate {
                kind: DuplicateKind::SingleHigh,
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                source: s.source(),
            })
        }
        22 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::StoreHalf {
                half: Half::High,
                source: s.vector(2),
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
            AvxSemantic::Move(if s.source_memory() {
                MoveInstruction::Load {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    aligned: true,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
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
            AvxSemantic::Move(if s.source_memory() {
                MoveInstruction::Load {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    aligned: true,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        25 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(if s.target_memory() {
                MoveInstruction::Store {
                    width: VectorWidth::Xmm,
                    source: s.vector(2),
                    aligned: true,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Xmm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        26 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(if s.target_memory() {
                MoveInstruction::Store {
                    width: VectorWidth::Ymm,
                    source: s.vector(2),
                    aligned: true,
                }
            } else {
                MoveInstruction::Copy {
                    width: VectorWidth::Ymm,
                    destination: s.vector(0),
                    source: s.vector(2),
                }
            })
        }
        27 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Store {
                width: VectorWidth::Xmm,
                source: s.vector(2),
                aligned: true,
            })
        }
        28 => {
            s.validate(
                B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Store {
                width: VectorWidth::Ymm,
                source: s.vector(2),
                aligned: true,
            })
        }
        29 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Mask {
                element: Element::Single,
                width: VectorWidth::Xmm,
                destination: s.general(0),
                source: s.vector(2),
            })
        }
        30 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Mask {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: s.general(0),
                source: s.vector(2),
            })
        }
        31 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Mask {
                element: Element::Double,
                width: VectorWidth::Xmm,
                destination: s.general(0),
                source: s.vector(2),
            })
        }
        32 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Mask {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: s.general(0),
                source: s.vector(2),
            })
        }
        33 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::DwordLoad {
                destination: s.vector(0),
                source: s.word_source(),
            })
        }
        34 => {
            s.validate(
                0,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::ZeroUpper)
        }
        35 => {
            s.validate(
                0,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::ZeroAll)
        }
        36 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::DwordStore {
                source: s.vector(2),
                destination: s.word_target(),
            })
        }
        37 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::QwordLoad {
                destination: s.vector(0),
                source: s.source(),
            })
        }
        38 => {
            s.validate(
                0,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::LoadControl)
        }
        39 => {
            s.validate(
                0,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Required,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::StoreControl)
        }
        40 => {
            s.validate(
                D | B,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Optional,
                false,
                immediate,
            )?;
            AvxSemantic::Move(if s.target_memory() {
                MoveInstruction::QwordStore {
                    source: s.vector(2),
                }
            } else {
                MoveInstruction::QwordLoad {
                    destination: s.vector(0),
                    source: MoveSource::Register(s.vector(2)),
                }
            })
        }
        41 => {
            s.validate(
                D | B,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Load {
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                aligned: false,
            })
        }
        42 => {
            s.validate(
                D | B,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Load {
                width: VectorWidth::Ymm,
                destination: s.vector(0),
                aligned: false,
            })
        }
        43 => {
            s.validate(
                D | B,
                MemoryPolicy::Required,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Move(MoveInstruction::Load {
                width: VectorWidth::Xmm,
                destination: s.vector(0),
                aligned: true,
            })
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
