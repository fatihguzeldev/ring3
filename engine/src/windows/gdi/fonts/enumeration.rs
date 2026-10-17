use super::{FontDatabase32, LogicalFont32, ResolvedFont32};
use crate::windows::gdi::GdiError32;
#[derive(Clone)]
pub struct EnumeratedFont32 {
    pub font: ResolvedFont32,
    pub full_name: Vec<u16>,
    pub style: Vec<u16>,
    pub script: &'static str,
    pub signature: [u32; 6],
    pub em: u32,
}
fn name(face: &rustybuzz::ttf_parser::Face<'_>, id: u16, limit: usize) -> Vec<u16> {
    face.names()
        .into_iter()
        .find_map(|name| (name.name_id == id).then(|| name.to_string()).flatten())
        .map_or_else(Vec::new, |name| name.encode_utf16().take(limit).collect())
}
fn script(charset: u8) -> &'static str {
    match charset {
        0 => "Western",
        2 => "Symbol",
        128 => "Japanese",
        129 => "Korean",
        130 => "Johab",
        134 => "Chinese GB2312",
        136 => "Chinese Big5",
        161 => "Greek",
        162 => "Turkish",
        163 => "Vietnamese",
        177 => "Hebrew",
        178 => "Arabic",
        186 => "Baltic",
        204 => "Cyrillic",
        222 => "Thai",
        238 => "Central European",
        _ => "Unicode",
    }
}
impl FontDatabase32 {
    pub fn enumerate_fonts(
        &self,
        filter: &LogicalFont32,
    ) -> Result<Vec<EnumeratedFont32>, GdiError32> {
        if filter.pitch_family != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let family = String::from_utf16_lossy(&filter.face);
        let mut result: Vec<EnumeratedFont32> = Vec::new();
        for font in self.enumerable_fonts()? {
            let actual = String::from_utf16_lossy(font.face_name());
            if !family.is_empty() && !actual.eq_ignore_ascii_case(&family) {
                continue;
            }
            if family.is_empty()
                && filter.charset == 1
                && result
                    .iter()
                    .any(|old| old.font.face_name() == font.face_name())
            {
                continue;
            }
            let face = rustybuzz::ttf_parser::Face::parse(&font.face.bytes, font.face.index)
                .map_err(|_| GdiError32::InvalidRequest)?;
            let os2 = face
                .raw_face()
                .table(rustybuzz::ttf_parser::Tag::from_bytes(b"OS/2"));
            let word = |at| {
                os2.and_then(|bytes| bytes.get(at..at + 4))
                    .map_or(0, |bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
            };
            let mut signature = [0; 6];
            for (index, at) in [42, 46, 50, 54, 78, 82].into_iter().enumerate() {
                signature[index] = word(at);
            }
            let mut charsets = Vec::new();
            for (bit, charset) in [
                (0, 0),
                (1, 238),
                (2, 204),
                (3, 161),
                (4, 162),
                (5, 177),
                (6, 178),
                (7, 186),
                (8, 163),
                (16, 222),
                (17, 128),
                (18, 134),
                (19, 129),
                (20, 136),
                (21, 130),
                (31, 2),
            ] {
                if signature[4] & (1 << bit) != 0 {
                    charsets.push(charset);
                }
            }
            if charsets.is_empty() {
                charsets.push(
                    if face.glyph_index('A').is_some() && face.glyph_index('z').is_some() {
                        0
                    } else {
                        1
                    },
                );
            }
            let full_name = name(&face, rustybuzz::ttf_parser::name_id::FULL_NAME, 63);
            let style = name(&face, rustybuzz::ttf_parser::name_id::SUBFAMILY, 31);
            let em = u32::from(face.units_per_em());
            for charset in charsets {
                if filter.charset != 1 && filter.charset != charset {
                    continue;
                }
                let mut selected = font.clone();
                selected.descriptor.charset = charset;
                result.try_reserve(1).map_err(|_| GdiError32::Allocation)?;
                result.push(EnumeratedFont32 {
                    font: selected,
                    full_name: full_name.clone(),
                    style: style.clone(),
                    script: script(charset),
                    signature,
                    em,
                });
                if family.is_empty() && filter.charset == 1 {
                    break;
                }
            }
        }
        Ok(result)
    }
}
