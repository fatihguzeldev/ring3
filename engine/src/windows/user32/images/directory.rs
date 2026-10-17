use super::{ImageExtent32, ImageKind32};
use crate::windows::user32::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageSource32 {
    Resource(u16),
    File {
        offset: u32,
        length: u32,
        hotspot: Option<(u16, u16)>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageEntry32 {
    pub extent: ImageExtent32,
    pub depth: u32,
    pub bytes: u32,
    pub source: ImageSource32,
}
pub(crate) struct ImageDirectory32<'a> {
    bytes: &'a [u8],
    kind: ImageKind32,
    count: usize,
    file: bool,
}
impl<'a> ImageDirectory32<'a> {
    pub(crate) fn resource(bytes: &'a [u8], kind: ImageKind32) -> Result<Self, E> {
        Self::new(bytes, kind, false)
    }
    pub(crate) fn file(bytes: &'a [u8], kind: ImageKind32) -> Result<Self, E> {
        Self::new(bytes, kind, true)
    }
    fn new(bytes: &'a [u8], kind: ImageKind32, file: bool) -> Result<Self, E> {
        if kind == ImageKind32::Bitmap || word(bytes, 0)? != 0 || word(bytes, 2)? != kind as u16 {
            return Err(E::Win32(13));
        }
        let count = usize::from(word(bytes, 4)?);
        let required = 6 + count * if file { 16 } else { 14 };
        if count == 0 || bytes.len() < required {
            return Err(E::Win32(13));
        }
        let directory = Self {
            bytes,
            kind,
            count,
            file,
        };
        for index in 0..count {
            directory.entry(index)?;
        }
        Ok(directory)
    }
    pub(crate) fn first(&self) -> ImageEntry32 {
        self.entry(0).unwrap()
    }
    pub(crate) fn select(
        &self,
        desired: Option<ImageExtent32>,
        depth: u32,
    ) -> Result<ImageEntry32, E> {
        let mut best = self.first();
        let mut best_key = selection_key(best, desired, depth, self.kind);
        for index in 1..self.count {
            let image = self.entry(index)?;
            let key = selection_key(image, desired, depth, self.kind);
            if key < best_key {
                best = image;
                best_key = key;
            }
        }
        Ok(best)
    }
    fn entry(&self, index: usize) -> Result<ImageEntry32, E> {
        let at = 6 + index * if self.file { 16 } else { 14 };
        let raw = self
            .bytes
            .get(at..at + if self.file { 16 } else { 14 })
            .ok_or(E::Win32(13))?;
        let (width, height) = if self.kind == ImageKind32::Cursor && !self.file {
            (
                dimension(u32::from(word(raw, 0)?)),
                dimension(u32::from(word(raw, 2)?)),
            )
        } else {
            if raw[3] != 0 {
                return Err(E::Win32(13));
            }
            (dimension(u32::from(raw[0])), dimension(u32::from(raw[1])))
        };
        let bytes = dword(raw, 8)?;
        if bytes == 0 {
            return Err(E::Win32(13));
        }
        let (source, depth) = if self.file {
            let offset = dword(raw, 12)?;
            let end = u64::from(offset) + u64::from(bytes);
            let directory_end = 6 + self.count * 16;
            if (offset as usize) < directory_end || end > self.bytes.len() as u64 {
                return Err(E::Win32(13));
            }
            let hotspot = (self.kind == ImageKind32::Cursor)
                .then(|| (word(raw, 4).unwrap(), word(raw, 6).unwrap()));
            let bits =
                if self.kind == ImageKind32::Cursor || word(raw, 4)? == 0 || word(raw, 6)? == 0 {
                    image_depth(&self.bytes[offset as usize..end as usize])?
                } else {
                    u32::from(word(raw, 4)?) * u32::from(word(raw, 6)?)
                };
            (
                ImageSource32::File {
                    offset,
                    length: bytes,
                    hotspot,
                },
                bits,
            )
        } else {
            (
                ImageSource32::Resource(word(raw, 12)?),
                u32::from(word(raw, 4)?) * u32::from(word(raw, 6)?),
            )
        };
        Ok(ImageEntry32 {
            extent: ImageExtent32 { width, height },
            depth,
            bytes,
            source,
        })
    }
}
fn dimension(value: u32) -> u32 {
    if value == 0 { 256 } else { value }
}
fn selection_key(
    image: ImageEntry32,
    desired: Option<ImageExtent32>,
    depth: u32,
    kind: ImageKind32,
) -> (u64, u64, u64, u64) {
    let deeper = u64::from(image.depth > depth);
    let depth_distance = u64::from(image.depth.abs_diff(depth));
    if kind == ImageKind32::Cursor {
        // native cursor groups choose depth before width and do not consult height.
        let distance = desired.map_or(0, |size| u64::from(image.extent.width.abs_diff(size.width)));
        let tie = desired.map_or(0, |_| u64::from(u32::MAX - image.extent.width));
        return (deeper, depth_distance, distance, tie);
    }
    let Some(desired) = desired else {
        return (0, 0, 0, 0);
    };
    let distance = u64::from(image.extent.width.abs_diff(desired.width))
        + u64::from(image.extent.height.abs_diff(desired.height));
    let tie = u64::MAX - u64::from(image.extent.width) * u64::from(image.extent.height);
    (distance, tie, deeper, depth_distance)
}
fn image_depth(bytes: &[u8]) -> Result<u32, E> {
    match dword(bytes, 0)? {
        12 => Ok(u32::from(word(bytes, 8)?) * u32::from(word(bytes, 10)?)),
        40..=124 => Ok(u32::from(word(bytes, 12)?) * u32::from(word(bytes, 14)?)),
        _ => Err(E::Win32(50)),
    }
}
fn word(bytes: &[u8], at: usize) -> Result<u16, E> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or(E::Win32(13))?
            .try_into()
            .unwrap(),
    ))
}
fn dword(bytes: &[u8], at: usize) -> Result<u32, E> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(E::Win32(13))?
            .try_into()
            .unwrap(),
    ))
}
