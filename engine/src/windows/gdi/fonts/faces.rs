use super::{super::GdiError32, LogicalFont32};
use std::sync::{Arc, OnceLock};

pub(super) struct FontDesign32 {
    pub units: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub average: f32,
    pub maximum: f32,
    pub weight: i32,
    pub default: u16,
    pub break_character: u16,
    pub first: u16,
    pub last: u16,
    pub monospace: bool,
    pub italic: bool,
}
pub(super) struct FontFace32 {
    pub bytes: Arc<[u8]>,
    pub index: u32,
    pub raster: fontdue::Font,
    pub name: Vec<u16>,
    pub design: FontDesign32,
}
pub struct FontDatabase32 {
    pub(super) faces: Vec<Arc<FontFace32>>,
    pub(super) resources: Vec<(u32, Vec<Arc<FontFace32>>)>,
    next_resource: u32,
    byte_limit: usize,
    bytes: usize,
}
static BUILTIN: OnceLock<Result<[Arc<FontFace32>; 2], GdiError32>> = OnceLock::new();
fn parse(bytes: Arc<[u8]>, index: u32) -> Result<Arc<FontFace32>, GdiError32> {
    let face = rustybuzz::ttf_parser::Face::parse(&bytes, index)
        .map_err(|_| GdiError32::InvalidRequest)?;
    if face.number_of_glyphs() == 0 || face.units_per_em() == 0 {
        return Err(GdiError32::InvalidRequest);
    }
    let name = face
        .names()
        .into_iter()
        .find_map(|name| {
            (name.name_id == rustybuzz::ttf_parser::name_id::FAMILY)
                .then(|| name.to_string())
                .flatten()
        })
        .ok_or(GdiError32::InvalidRequest)?;
    if name.encode_utf16().count() >= 32 {
        return Err(GdiError32::Unsupported);
    }
    let average = face
        .raw_face()
        .table(rustybuzz::ttf_parser::Tag::from_bytes(b"OS/2"))
        .and_then(|bytes| bytes.get(2..4))
        .map(|bytes| i16::from_be_bytes(bytes.try_into().unwrap()))
        .filter(|value| *value > 0)
        .map(f32::from)
        .unwrap_or_else(|| {
            let mut sum = 0u64;
            for glyph in 0..face.number_of_glyphs() {
                sum += u64::from(
                    face.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(glyph))
                        .unwrap_or(0),
                );
            }
            sum as f32 / f32::from(face.number_of_glyphs())
        });
    let maximum = face
        .raw_face()
        .table(rustybuzz::ttf_parser::Tag::from_bytes(b"hhea"))
        .and_then(|bytes| bytes.get(10..12))
        .map_or(average, |bytes| {
            f32::from(u16::from_be_bytes(bytes.try_into().unwrap()))
        });
    let os2 = face
        .raw_face()
        .table(rustybuzz::ttf_parser::Tag::from_bytes(b"OS/2"));
    let character = |offset, fallback| {
        os2.filter(|_| face.tables().os2.is_some_and(|table| table.version >= 2))
            .and_then(|table| table.get(offset..offset + 2))
            .map_or(fallback, |bytes| {
                u16::from_be_bytes(bytes.try_into().unwrap())
            })
    };
    let mut first = u16::MAX;
    let mut last = 0;
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if subtable.is_unicode() {
                subtable.codepoints(|code| {
                    if let Ok(code) = u16::try_from(code) {
                        first = first.min(code);
                        last = last.max(code);
                    }
                });
            }
        }
    }
    let design = FontDesign32 {
        units: f32::from(face.units_per_em()),
        ascent: f32::from(face.ascender()),
        descent: -f32::from(face.descender()),
        line_gap: f32::from(face.line_gap()),
        average,
        maximum,
        weight: i32::from(face.weight().to_number()),
        default: character(90, 0),
        break_character: character(92, 32),
        first,
        last,
        monospace: face.is_monospaced(),
        italic: face.is_italic(),
    };
    if design.ascent + design.descent <= 0.0 || design.average <= 0.0 {
        return Err(GdiError32::InvalidRequest);
    }
    let raster = fontdue::Font::from_bytes(
        bytes.as_ref(),
        fontdue::FontSettings {
            collection_index: index,
            ..Default::default()
        },
    )
    .map_err(|_| GdiError32::InvalidRequest)?;
    Ok(Arc::new(FontFace32 {
        bytes,
        index,
        raster,
        name: name.encode_utf16().collect(),
        design,
    }))
}
impl FontDatabase32 {
    pub fn new(byte_limit: usize) -> Result<Self, GdiError32> {
        if !(1_082_044..=268_435_456).contains(&byte_limit) {
            return Err(GdiError32::InvalidConfig);
        }
        let builtin = BUILTIN
            .get_or_init(|| {
                Ok([
                    parse(
                        Arc::from(
                            include_bytes!("../../../../assets/fonts/NotoSans-Regular.ttf")
                                .as_slice(),
                        ),
                        0,
                    )?,
                    parse(
                        Arc::from(
                            include_bytes!("../../../../assets/fonts/NotoSansMono-Regular.ttf")
                                .as_slice(),
                        ),
                        0,
                    )?,
                ])
            })
            .as_ref()
            .map_err(|error| *error)?;
        Ok(Self {
            faces: builtin.to_vec(),
            resources: Vec::new(),
            next_resource: 1,
            byte_limit,
            bytes: 1_082_044,
        })
    }
    pub fn add_memory_font(&mut self, bytes: &[u8]) -> Result<u32, GdiError32> {
        let count = rustybuzz::ttf_parser::fonts_in_collection(bytes).unwrap_or(1) as usize;
        if count == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        if bytes.len() > self.byte_limit - self.bytes
            || self.resources.len() == 256
            || count > 256usize.saturating_sub(self.faces.len() - 2)
        {
            return Err(GdiError32::Capacity);
        }
        let next = self
            .next_resource
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        let source: Arc<[u8]> = Arc::from(bytes);
        let mut added = Vec::new();
        added
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        for index in 0..count {
            added.push(parse(source.clone(), index as u32)?);
        }
        self.resources
            .try_reserve(1)
            .map_err(|_| GdiError32::Allocation)?;
        self.faces
            .try_reserve(count)
            .map_err(|_| GdiError32::Allocation)?;
        let id = self.next_resource;
        self.faces.extend(added.iter().cloned());
        self.resources.push((id, added));
        self.bytes += bytes.len();
        self.next_resource = next;
        Ok(id)
    }
    pub fn remove_memory_font(&mut self, id: u32) -> Result<(), GdiError32> {
        let index = self
            .resources
            .iter()
            .position(|entry| entry.0 == id)
            .ok_or(GdiError32::InvalidHandle)?;
        let (_, faces) = self.resources.remove(index);
        self.bytes -= faces[0].bytes.len();
        self.faces
            .retain(|entry| !faces.iter().any(|face| Arc::ptr_eq(entry, face)));
        Ok(())
    }
    pub(super) fn face(&self, font: &LogicalFont32) -> &FontFace32 {
        let requested = String::from_utf16_lossy(&font.face);
        self.faces
            .iter()
            .rev()
            .find(|face| String::from_utf16_lossy(&face.name).eq_ignore_ascii_case(&requested))
            .unwrap_or_else(|| {
                &self.faces[usize::from(
                    font.pitch_family & 3 == 1
                        || requested.to_ascii_lowercase().contains("mono")
                        || requested.eq_ignore_ascii_case("Courier New"),
                )]
            })
    }
    pub fn face_name(&self, font: &LogicalFont32) -> &[u16] {
        &self.face(font).name
    }
    pub fn glyph_index(&self, font: &LogicalFont32, character: char) -> u16 {
        self.face(font).raster.lookup_glyph_index(character)
    }
    pub fn glyph_count(&self, font: &LogicalFont32) -> u16 {
        self.face(font).raster.glyph_count()
    }
}
