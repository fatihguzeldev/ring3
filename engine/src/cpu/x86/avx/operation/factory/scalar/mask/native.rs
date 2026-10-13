use super::*;

#[test]
fn only_the_mask_kind_gains_width_and_keeps_exact_slot_and_immediate_validation() {
    for operands in 0..=0x3fff {
        for immediate in [0, 1, 255] {
            let narrow = AvxInstruction::decode_descriptor(0x01000903, operands, immediate);
            let wide = AvxInstruction::decode_descriptor(0x01000983, operands, immediate);
            match (narrow, wide) {
                (Ok(narrow), Ok(wide)) => {
                    assert_eq!(narrow.encode(), (0x01000903, operands, 0));
                    assert_eq!(wide.encode(), (0x01000983, operands, 0));
                    let AvxSemantic::Scalar(ScalarInstruction::Mask {
                        width: a,
                        source: a_source,
                        destination: a_destination,
                    }) = narrow.semantic()
                    else {
                        panic!("narrow mask family")
                    };
                    let AvxSemantic::Scalar(ScalarInstruction::Mask {
                        width: b,
                        source: b_source,
                        destination: b_destination,
                    }) = wide.semantic()
                    else {
                        panic!("wide mask family")
                    };
                    assert_eq!(a, VectorWidth::Xmm);
                    assert_eq!(b, VectorWidth::Ymm);
                    assert_eq!(a_source, b_source);
                    assert_eq!(a_destination, b_destination);
                    assert_eq!(wide.memory(), MemoryEffect::None);
                    assert_eq!(wide.inputs(), [None, None]);
                    assert_eq!(wide.output(), Some(b_destination));
                    assert!(!wide.writes_flags());
                }
                (Err(narrow), Err(wide)) => assert_eq!(narrow, wide),
                pair => panic!("slots{operands:04x} immediate{immediate}: {pair:?}"),
            }
        }
    }
    for kind in [0x80, 0x81, 0x82, 0x84, 0x85, 0x86, 0x87, 0x88, 0xc3, 0xff] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01000900 | kind, 0, 0),
            Err(AvxDecodeError::Family)
        );
    }
}
