use crate::windows::nls::NlsError32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocaleIdentity32 {
    pub lcid: u32,
    pub name: &'static str,
    pub english_name: &'static str,
    pub language: &'static str,
    pub language_three_letter: &'static str,
    pub windows_three_letter: &'static str,
    pub ansi: u32,
    collation: Option<&'static str>,
}
impl LocaleIdentity32 {
    pub fn windows_name(self) -> &'static str {
        match self.lcid {
            0x040a => "es-ES_tradnl",
            0x10407 => "de-DE_phoneb",
            0x1040e => "hu-HU_technl",
            0x10437 => "ka-GE_modern",
            0x20804 => "zh-CN_stroke",
            0x30404 => "zh-TW_pronun",
            0x40404 => "zh-TW_radstr",
            0x40c04 => "zh-HK_radstr",
            0x41404 => "zh-MO_radstr",
            0x21004 => "zh-SG_stroke",
            0x40411 => "ja-JP_radstr",
            _ => self.name,
        }
    }
    pub fn alternate_sort(self) -> bool {
        self.collation.is_some()
    }
    pub fn sort_name(self) -> &'static str {
        match self.collation {
            Some("trad") => "Traditional",
            Some("phonebk") => "Phone Book",
            Some("technl") => "Technical",
            Some("modern") => "Modern",
            Some("stroke") => "Stroke Count",
            Some("zhuyin") => "Pronunciation",
            Some("unihan") => "Radical/Stroke",
            _ => "Default",
        }
    }
    pub fn language_tag(self) -> String {
        let name = if self.name.is_empty() {
            "und"
        } else {
            self.name
        };
        if let Some(collation) = self.collation {
            format!("{name}-u-co-{collation}")
        } else {
            name.into()
        }
    }
}
pub fn locale_identity(lcid: u32) -> Result<LocaleIdentity32, NlsError32> {
    let (base, collation) = match lcid {
        0x040a => (0x040a, Some("trad")),
        0x10407 => (0x407, Some("phonebk")),
        0x1040e => (0x40e, Some("technl")),
        0x10437 => (0x437, Some("modern")),
        0x20804 => (0x804, Some("stroke")),
        0x30404 => (0x404, Some("zhuyin")),
        0x40404 => (0x404, Some("unihan")),
        0x40c04 => (0xc04, Some("unihan")),
        0x41404 => (0x1404, Some("unihan")),
        0x21004 => (0x1004, Some("stroke")),
        0x40411 => (0x411, Some("unihan")),
        _ => (lcid, None),
    };
    let value: &'static lcid::LanguageId =
        base.try_into().map_err(|_| NlsError32::InvalidParameter)?;
    Ok(LocaleIdentity32 {
        lcid,
        name: value.name,
        english_name: value.english_name,
        language: value.iso639_two_letter,
        language_three_letter: value.iso639_three_letter,
        windows_three_letter: value.windows_three_letter,
        ansi: value.ansi_code_page.map_or(0, u32::from),
        collation,
    })
}
