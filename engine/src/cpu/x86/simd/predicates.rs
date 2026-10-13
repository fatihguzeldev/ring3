#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitTestKind {
    Bits,
    SingleSign,
    DoubleSign,
}

pub(crate) fn flags(first: &[u8], second: &[u8], kind: BitTestKind, incoming: u32) -> u32 {
    let mut and = 0u8;
    let mut and_not = 0u8;
    for (index, (&left, &right)) in first.iter().zip(second).enumerate() {
        let selected = match kind {
            BitTestKind::Bits => 0xff,
            BitTestKind::SingleSign if index % 4 == 3 => 0x80,
            BitTestKind::DoubleSign if index % 8 == 7 => 0x80,
            _ => 0,
        };
        and |= left & right & selected;
        and_not |= !left & right & selected;
    }
    (incoming & !0x8d5) | (u32::from(and == 0) << 6) | u32::from(and_not == 0)
}
