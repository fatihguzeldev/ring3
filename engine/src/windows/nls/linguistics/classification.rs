use crate::windows::nls::NlsError32;
use unicode_general_category::{GeneralCategory as G, get_general_category as category};
fn punctuation(c: G) -> bool {
    matches!(
        c,
        G::ClosePunctuation
            | G::ConnectorPunctuation
            | G::DashPunctuation
            | G::FinalPunctuation
            | G::InitialPunctuation
            | G::OpenPunctuation
            | G::OtherPunctuation
    )
}
fn flag(value: bool, bit: u16) -> u16 {
    u16::from(value) * bit
}
pub fn character_types(units: &[u16], kind: u32) -> Result<Vec<u16>, NlsError32> {
    if !(1..=3).contains(&kind) {
        return Err(NlsError32::InvalidFlags);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(units.len())
        .map_err(|_| NlsError32::Allocation)?;
    for &unit in units {
        let value = char::from_u32(u32::from(unit));
        let c = value.map_or(G::Surrogate, category);
        let alpha = value.is_some_and(char::is_alphabetic);
        let bits = match kind {
            1 => {
                flag(c == G::UppercaseLetter, 1)
                    | flag(c == G::LowercaseLetter, 2)
                    | flag(c == G::DecimalNumber, 4)
                    | flag(value.is_some_and(char::is_whitespace), 8)
                    | flag(
                        punctuation(c)
                            || matches!(
                                c,
                                G::CurrencySymbol
                                    | G::MathSymbol
                                    | G::ModifierSymbol
                                    | G::OtherSymbol
                            ),
                        0x10,
                    )
                    | flag(c == G::Control, 0x20)
                    | flag(c == G::SpaceSeparator || unit == 9, 0x40)
                    | flag(value.is_some_and(|c| c.is_ascii_hexdigit()), 0x80)
                    | flag(alpha, 0x100)
                    | flag(c != G::Unassigned && c != G::Surrogate, 0x200)
            }
            2 => value.map_or(0, |value| match unicode_bidi::bidi_class(value) {
                unicode_bidi::BidiClass::L => 1,
                unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => 2,
                unicode_bidi::BidiClass::EN => 3,
                unicode_bidi::BidiClass::ES => 4,
                unicode_bidi::BidiClass::ET => 5,
                unicode_bidi::BidiClass::AN => 6,
                unicode_bidi::BidiClass::CS => 7,
                unicode_bidi::BidiClass::B => 8,
                unicode_bidi::BidiClass::S => 9,
                unicode_bidi::BidiClass::WS => 10,
                unicode_bidi::BidiClass::ON => 11,
                _ => 0,
            }),
            _ => {
                flag(c == G::NonspacingMark, 1)
                    | flag(matches!(c, G::NonspacingMark | G::SpacingMark), 2)
                    | flag(c == G::SpacingMark, 4)
                    | flag(
                        matches!(
                            c,
                            G::CurrencySymbol | G::MathSymbol | G::ModifierSymbol | G::OtherSymbol
                        ),
                        8,
                    )
                    | flag((0x30a0..=0x30ff).contains(&unit), 0x10)
                    | flag((0x3040..=0x309f).contains(&unit), 0x20)
                    | flag((0xff61..=0xffdc).contains(&unit), 0x40)
                    | flag((0xff01..=0xff60).contains(&unit), 0x80)
                    | flag((0x3400..=0x9fff).contains(&unit), 0x100)
                    | flag(unit == 0x640, 0x200)
                    | flag(matches!(unit, 0x2d | 0x3d | 0xaa | 0xba | 0x640), 0x400)
                    | flag((0xd800..=0xdbff).contains(&unit), 0x800)
                    | flag((0xdc00..=0xdfff).contains(&unit), 0x1000)
                    | flag(alpha, 0x8000)
            }
        };
        out.push(bits);
    }
    Ok(out)
}
