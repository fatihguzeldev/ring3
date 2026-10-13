use super::*;
use crate::memory::Access;

fn error(kind: u32, operands: u32, immediate: u32) -> Option<AvxDecodeError> {
    let immediate_count = kind <= 10;
    let bad_operands = if immediate_count {
        operands & !0x3f != 0
    } else {
        operands & ((7 << 9) | (1 << 13)) != 0
            || operands & (1 << 12) != 0 && operands & (7 << 6) != 0
    };
    if bad_operands {
        Some(AvxDecodeError::Operands)
    } else if !immediate_count && immediate != 0 {
        Some(AvxDecodeError::Immediate)
    } else {
        None
    }
}

#[test]
fn all_eighteen_widths_preserve_historic_slots_and_immediate_priority() {
    for kind in 1..=18 {
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let mut operation = None;
                for (width, bit) in [(VectorWidth::Xmm, 0), (VectorWidth::Ymm, 0x80)] {
                    let descriptor = 0x01000700 | kind | bit;
                    let actual = AvxInstruction::decode_descriptor(descriptor, operands, immediate);
                    if let Some(error) = error(kind, operands, immediate) {
                        assert_eq!(actual, Err(error));
                        continue;
                    }
                    let value = actual.unwrap();
                    assert_eq!(value.encode(), (descriptor, operands, immediate));
                    let AvxSemantic::Shift(instruction) = value.semantic() else {
                        panic!("shift family");
                    };
                    assert_eq!(instruction.width(), width);
                    assert_eq!(instruction.destination().index(), (operands & 7) as usize);
                    assert_eq!(instruction.source().index(), ((operands >> 3) & 7) as usize);
                    if let Some(narrow) = operation {
                        assert_eq!(instruction.operation(), narrow);
                    }
                    operation = Some(instruction.operation());
                    let expected_count = if kind <= 10 {
                        ShiftCount::Immediate(immediate as u8)
                    } else if operands & (1 << 12) != 0 {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(VectorRegister(((operands >> 6) & 7) as u8))
                    };
                    assert_eq!(instruction.count(), expected_count);
                    assert_eq!(
                        value.memory(),
                        if expected_count == ShiftCount::Memory {
                            MemoryEffect::Full(MemoryRequirement {
                                access: Access::Read,
                                bytes: 16,
                                alignment: 1,
                            })
                        } else {
                            MemoryEffect::None
                        }
                    );
                    assert_eq!(value.inputs(), [None, None]);
                    assert_eq!(value.output(), None);
                    assert!(!value.writes_flags());
                }
            }
        }
    }
    for kind in (0..=255).filter(|kind| !(1..=18).contains(kind) && !(0x81..=0x92).contains(kind)) {
        for operands in [0, 1 << 9, 1 << 12, 1 << 13, 0x3fff] {
            for immediate in [0, 1, 255] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(0x01000700 | kind, operands, immediate),
                    Err(AvxDecodeError::Family)
                );
            }
        }
    }
}
