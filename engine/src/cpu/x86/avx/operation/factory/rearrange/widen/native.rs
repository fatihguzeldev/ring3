use super::*;

#[test]
fn width_bit_preserves_all_legacy_slots_and_only_twelve_widening_kinds() {
    for kind in [16, 17, 18, 19, 20, 21, 23, 24, 25, 26, 27, 28] {
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let narrow =
                    AvxInstruction::decode_descriptor(0x01000600 | kind, operands, immediate);
                let wide =
                    AvxInstruction::decode_descriptor(0x01000680 | kind, operands, immediate);
                match (narrow, wide) {
                    (Ok(narrow), Ok(wide)) => {
                        assert_eq!(narrow.encode(), (0x01000600 | kind, operands, 0));
                        assert_eq!(wide.encode(), (0x01000680 | kind, operands, 0));
                        let AvxSemantic::Rearrange(a) = narrow.semantic() else {
                            panic!("rearrange family")
                        };
                        let AvxSemantic::Rearrange(b) = wide.semantic() else {
                            panic!("rearrange family")
                        };
                        assert_eq!(a.width(), VectorWidth::Xmm);
                        assert_eq!(b.width(), VectorWidth::Ymm);
                        assert_eq!(b.with_width(VectorWidth::Xmm), Some(a));
                        assert_eq!(b.source_bytes(), a.source_bytes() * 2);
                        assert_eq!(wide.inputs(), [None, None]);
                        assert_eq!(wide.output(), None);
                        assert!(!wide.writes_flags());
                    }
                    (Err(narrow), Err(wide)) => assert_eq!(narrow, wide),
                    pair => panic!("kind{kind} slots{operands:04x} immediate{immediate}: {pair:?}"),
                }
            }
        }
    }
    for kind in [0, 32, 63, 64, 80, 127] {
        for bit in [0, 0x80] {
            assert_eq!(
                AvxInstruction::decode_descriptor(0x01000600 | kind | bit, 0, 0),
                Err(AvxDecodeError::Family)
            );
        }
    }
}
