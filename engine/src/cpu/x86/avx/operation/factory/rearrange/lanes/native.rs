use super::*;

#[test]
fn lane_widths_preserve_all_nineteen_slot_immediate_and_mask_policies() {
    for kind in (1..=15).chain([22, 29, 30, 31]) {
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let narrow =
                    AvxInstruction::decode_descriptor(0x01000600 | kind, operands, immediate);
                let wide =
                    AvxInstruction::decode_descriptor(0x01000680 | kind, operands, immediate);
                match (narrow, wide) {
                    (Ok(narrow), Ok(wide)) => {
                        assert_eq!(narrow.encode(), (0x01000600 | kind, operands, immediate));
                        assert_eq!(wide.encode(), (0x01000680 | kind, operands, immediate));
                        let AvxSemantic::Rearrange(a) = narrow.semantic() else {
                            panic!("rearrange family")
                        };
                        let AvxSemantic::Rearrange(b) = wide.semantic() else {
                            panic!("rearrange family")
                        };
                        assert_eq!(b.with_width(VectorWidth::Xmm), Some(a));
                        assert_eq!(b.source_bytes(), 32);
                        assert_eq!(a.source_bytes(), 16);
                        assert_eq!(wide.inputs(), [None, None]);
                        assert_eq!(wide.output(), None);
                        assert!(!wide.writes_flags());
                    }
                    (Err(narrow), Err(wide)) => assert_eq!(narrow, wide),
                    pair => panic!("kind{kind} slots{operands:04x} imm{immediate}: {pair:?}"),
                }
            }
        }
    }
}
