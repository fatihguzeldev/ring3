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
                u16::from(c == G::UppercaseLetter)
                    | u16::from(c == G::LowercaseLetter) * 2
                    | u16::from(c == G::DecimalNumber) * 4
                    | u16::from(value.is_some_and(char::is_whitespace)) * 8
                    | u16::from(
                        punctuation(c)
                            || matches!(
                                c,
                                G::CurrencySymbol
                                    | G::MathSymbol
                                    | G::ModifierSymbol
                                    | G::OtherSymbol
                            ),
                    ) * 0x10
                    | u16::from(c == G::Control) * 0x20
                    | u16::from(c == G::SpaceSeparator || unit == 9) * 0x40
                    | u16::from(value.is_some_and(|c| c.is_ascii_hexdigit())) * 0x80
                    | u16::from(alpha) * 0x100
                    | u16::from(c != G::Unassigned && c != G::Surrogate) * 0x200
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
                u16::from(c == G::NonspacingMark)
                    | u16::from(matches!(c, G::NonspacingMark | G::SpacingMark)) * 2
                    | u16::from(c == G::SpacingMark) * 4
                    | u16::from(matches!(
                        c,
                        G::CurrencySymbol | G::MathSymbol | G::ModifierSymbol | G::OtherSymbol
                    )) * 8
                    | u16::from((0x30a0..=0x30ff).contains(&unit)) * 0x10
                    | u16::from((0x3040..=0x309f).contains(&unit)) * 0x20
                    | u16::from((0xff61..=0xffdc).contains(&unit)) * 0x40
                    | u16::from((0xff01..=0xff60).contains(&unit)) * 0x80
                    | u16::from((0x3400..=0x9fff).contains(&unit)) * 0x100
                    | u16::from(unit == 0x640) * 0x200
                    | u16::from(matches!(unit, 0x2d | 0x3d | 0xaa | 0xba | 0x640)) * 0x400
                    | u16::from((0xd800..=0xdbff).contains(&unit)) * 0x800
                    | u16::from((0xdc00..=0xdfff).contains(&unit)) * 0x1000
                    | u16::from(alpha) * 0x8000
            }
        };
        out.push(bits);
    }
    Ok(out)
}
