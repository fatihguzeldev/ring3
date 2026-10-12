use ring3_engine::cpu::{
    UnsupportedFeature,
    x86::{
        SegmentRegister,
        decode::{DecodeError, DecodedInstruction},
        ir::{AddressSize, Operation, RepeatCondition, StringKind, StringWidth},
    },
};

// literal opcode/prefix oracle for the formerly refused string test rows.
// non-string neighbors retain their old opcode-refusal assertion.
pub fn check(decoded: Result<DecodedInstruction, DecodeError>, bytes: &[u8]) {
    let prefixes = &bytes[..bytes.len() - 1];
    for group in [&[0x66][..], &[0xf0][..], &[0xf2, 0xf3][..]] {
        if prefixes
            .iter()
            .filter(|prefix| group.contains(prefix))
            .count()
            > 1
        {
            assert_eq!(
                decoded.unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
            return;
        }
    }
    let opcode = *bytes.last().unwrap();
    let kind = match opcode {
        0xa4 | 0xa5 => StringKind::Move,
        0xaa | 0xab => StringKind::Store,
        0xac | 0xad => StringKind::Load,
        0xa6 | 0xa7 => StringKind::Compare,
        0xae | 0xaf => StringKind::Scan,
        _ => {
            assert_eq!(
                decoded.unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
            return;
        }
    };
    let width = if opcode & 1 == 0 {
        StringWidth::Byte
    } else if bytes.contains(&0x66) {
        StringWidth::Word
    } else {
        StringWidth::Dword
    };
    let address_size = if bytes.contains(&0x67) {
        AddressSize::Bits16
    } else {
        AddressSize::Bits32
    };
    let source_segment = bytes[..bytes.len() - 1]
        .iter()
        .filter_map(|prefix| match *prefix {
            0x26 => Some(Some(SegmentRegister::Es)),
            0x2e => Some(Some(SegmentRegister::Cs)),
            0x36 => Some(Some(SegmentRegister::Ss)),
            0x3e => Some(None),
            0x64 => Some(Some(SegmentRegister::Fs)),
            0x65 => Some(Some(SegmentRegister::Gs)),
            _ => None,
        })
        .next_back()
        .flatten();
    let repeat = bytes
        .iter()
        .rev()
        .find(|byte| matches!(**byte, 0xf2 | 0xf3));
    let expected = match repeat {
        Some(prefix) => Operation::RepeatString {
            kind,
            width,
            address_size,
            source_segment,
            repeat: if matches!(kind, StringKind::Compare | StringKind::Scan) {
                if *prefix == 0xf2 {
                    RepeatCondition::NotEqual
                } else {
                    RepeatCondition::Equal
                }
            } else {
                RepeatCondition::Always
            },
        },
        None => Operation::String {
            kind,
            width,
            address_size,
            source_segment,
        },
    };
    let decoded = decoded.unwrap();
    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
    assert_eq!(usize::from(decoded.length()), bytes.len());
}
