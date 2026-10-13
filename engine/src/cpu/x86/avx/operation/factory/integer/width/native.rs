use super::*;
use crate::memory::Access;

#[test]
fn all_widths_preserve_the_closed_kind_and_operand_policy() {
    for kind in 1..=44 {
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let narrow =
                    AvxInstruction::decode_descriptor(0x01000500 | kind, operands, immediate);
                let wide =
                    AvxInstruction::decode_descriptor(0x01000580 | kind, operands, immediate);
                match (narrow, wide) {
                    (Ok(narrow), Ok(wide)) => {
                        assert_eq!(narrow.encode(), (0x01000500 | kind, operands, 0));
                        assert_eq!(wide.encode(), (0x01000580 | kind, operands, 0));
                        let AvxSemantic::Integer(a) = narrow.semantic() else {
                            panic!("narrow integer family");
                        };
                        let AvxSemantic::Integer(b) = wide.semantic() else {
                            panic!("wide integer family");
                        };
                        assert_eq!(a.width(), VectorWidth::Xmm);
                        assert_eq!(b.width(), VectorWidth::Ymm);
                        assert_eq!(a.operation(), b.operation());
                        assert_eq!(a.destination(), b.destination());
                        assert_eq!(a.first(), b.first());
                        assert_eq!(a.second(), b.second());
                        for (value, bytes) in [(narrow, 16), (wide, 32)] {
                            assert_eq!(
                                value.memory(),
                                if operands & (1 << 12) != 0 {
                                    MemoryEffect::Full(MemoryRequirement {
                                        access: Access::Read,
                                        bytes,
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
                    (Err(narrow), Err(wide)) => assert_eq!(narrow, wide),
                    (narrow, wide) => panic!(
                        "kind{kind} slots{operands:04x} immediate{immediate}: {narrow:?} {wide:?}"
                    ),
                }
            }
        }
    }
    for kind in [0, 45, 63, 64, 65, 108, 127] {
        for width in [0, 0x80] {
            for operands in [0, 1 << 9, 1 << 12, 1 << 13, 0x3fff] {
                for immediate in [0, 1, 255] {
                    assert_eq!(
                        AvxInstruction::decode_descriptor(
                            0x01000500 | kind | width,
                            operands,
                            immediate
                        ),
                        Err(AvxDecodeError::Family)
                    );
                }
            }
        }
    }
}
