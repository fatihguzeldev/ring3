use super::{ImageDirectory32, ImageExtent32, ImageKind32, ImageSource32, LoadImage32};
use crate::windows::user32::User32Error32 as E;

pub(crate) struct FileImage32<'a> {
    pub info: &'a [u8],
    pub pixels: Option<&'a [u8]>,
    pub hotspot: (u16, u16),
    pub extent: ImageExtent32,
}
impl<'a> FileImage32<'a> {
    pub(crate) fn select(
        bytes: &'a [u8],
        request: LoadImage32,
        defaults: ImageExtent32,
        depth: u32,
    ) -> Result<Self, E> {
        if bytes.get(..4) == Some(&b"RIFF"[..]) && bytes.get(8..12) == Some(&b"ACON"[..]) {
            return Err(E::Win32(50));
        }
        if request.kind == ImageKind32::Bitmap {
            let bitmap = BitmapFile32::parse(bytes)?;
            return Ok(Self {
                info: bitmap.dib,
                pixels: Some(bitmap.pixels),
                hotspot: (0, 0),
                extent: ImageExtent32 {
                    width: request.width,
                    height: request.height,
                },
            });
        }
        let directory = ImageDirectory32::file(bytes, request.kind)?;
        let desired = request.selection(directory.first().extent, defaults);
        let entry = directory.select(desired, if request.monochrome() { 1 } else { depth })?;
        let ImageSource32::File {
            offset,
            length,
            hotspot,
        } = entry.source
        else {
            unreachable!()
        };
        let end = offset as usize + length as usize;
        Ok(Self {
            info: &bytes[offset as usize..end],
            pixels: None,
            hotspot: hotspot.unwrap_or((0, 0)),
            extent: request.requested_extent(defaults),
        })
    }
}

pub(crate) struct BitmapFile32<'a> {
    pub dib: &'a [u8],
    pub pixels: &'a [u8],
}
impl<'a> BitmapFile32<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, E> {
        if bytes.get(..2) != Some(&b"BM"[..]) || bytes.len() < 26 {
            return Err(E::Win32(13));
        }
        let length = dword(bytes, 2)? as usize;
        let offset = dword(bytes, 10)? as usize;
        let header = dword(bytes, 14)? as usize;
        if length != 0 && length > bytes.len()
            || offset < 14usize.checked_add(header).ok_or(E::Win32(13))?
            || offset > bytes.len()
        {
            return Err(E::Win32(13));
        }
        Ok(Self {
            dib: &bytes[14..],
            pixels: &bytes[offset..],
        })
    }
}

pub(crate) fn cursor_resource(bytes: &[u8]) -> Result<((u16, u16), &[u8]), E> {
    if bytes.len() < 8 {
        return Err(E::Win32(13));
    }
    Ok((
        (
            u16::from_le_bytes(bytes[..2].try_into().unwrap()),
            u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
        ),
        &bytes[4..],
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
