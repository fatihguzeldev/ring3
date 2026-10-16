use crate::windows::nls::NlsError32;
use icu_calendar::{AnyCalendar, AnyCalendarKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarId32 {
    Gregorian,
    GregorianUs,
    Japanese,
    Taiwan,
    Korean,
    Hijri,
    Thai,
    Hebrew,
    GregorianFrench,
    GregorianArabic,
    GregorianTransliteratedEnglish,
    GregorianTransliteratedFrench,
    Persian,
    UmAlQura,
}
impl CalendarId32 {
    pub fn from_id(id: u32) -> Result<Self, NlsError32> {
        Ok(match id {
            1 => Self::Gregorian,
            2 => Self::GregorianUs,
            3 => Self::Japanese,
            4 => Self::Taiwan,
            5 => Self::Korean,
            6 => Self::Hijri,
            7 => Self::Thai,
            8 => Self::Hebrew,
            9 => Self::GregorianFrench,
            10 => Self::GregorianArabic,
            11 => Self::GregorianTransliteratedEnglish,
            12 => Self::GregorianTransliteratedFrench,
            22 => Self::Persian,
            23 => Self::UmAlQura,
            _ => return Err(NlsError32::InvalidParameter),
        })
    }
    pub fn id(self) -> u32 {
        match self {
            Self::Gregorian => 1,
            Self::GregorianUs => 2,
            Self::Japanese => 3,
            Self::Taiwan => 4,
            Self::Korean => 5,
            Self::Hijri => 6,
            Self::Thai => 7,
            Self::Hebrew => 8,
            Self::GregorianFrench => 9,
            Self::GregorianArabic => 10,
            Self::GregorianTransliteratedEnglish => 11,
            Self::GregorianTransliteratedFrench => 12,
            Self::Persian => 22,
            Self::UmAlQura => 23,
        }
    }
    pub fn naming_locale(self, locale: u32) -> u32 {
        match self {
            Self::GregorianUs => 0x409,
            Self::GregorianFrench => 0x40c,
            Self::GregorianArabic => 0x801,
            Self::GregorianTransliteratedEnglish => 0xc01,
            Self::GregorianTransliteratedFrench => 0x1401,
            _ => locale,
        }
    }
    pub(super) fn calendar(self) -> AnyCalendar {
        AnyCalendar::new(match self {
            Self::Gregorian
            | Self::GregorianUs
            | Self::Korean
            | Self::GregorianFrench
            | Self::GregorianArabic
            | Self::GregorianTransliteratedEnglish
            | Self::GregorianTransliteratedFrench => AnyCalendarKind::Gregorian,
            Self::Japanese => AnyCalendarKind::Japanese,
            Self::Taiwan => AnyCalendarKind::Roc,
            Self::Hijri => AnyCalendarKind::HijriTabularTypeIIFriday,
            Self::Thai => AnyCalendarKind::Buddhist,
            Self::Hebrew => AnyCalendarKind::Hebrew,
            Self::Persian => AnyCalendarKind::Persian,
            Self::UmAlQura => AnyCalendarKind::HijriUmmAlQura,
        })
    }
}
