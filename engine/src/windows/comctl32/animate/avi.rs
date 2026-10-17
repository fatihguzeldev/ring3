use crate::windows::{
    gdi::{BitmapLayout32, decode_dib_header},
    user32::User32Error32 as E,
};
use std::ops::Range;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AviClip32 {
    pub layout: BitmapLayout32,
    pub colors: Vec<u32>,
    pub frames: Vec<Range<usize>>,
    pub period_ms: u32,
    bytes: Vec<u8>,
}
fn word(bytes: &[u8], at: usize) -> Result<u32, E> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(E::Win32(13))?
            .try_into()
            .unwrap(),
    ))
}
struct Chunk32<'a> {
    id: &'a [u8],
    data: &'a [u8],
    start: usize,
}
fn chunks(
    bytes: &[u8],
    start: usize,
    end: usize,
    mut read: impl FnMut(Chunk32<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let mut cursor = start;
    while cursor < end {
        let head = bytes
            .get(cursor..cursor + 8)
            .filter(|_| cursor + 8 <= end)
            .ok_or(E::Win32(13))?;
        let length = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let finish = (cursor + 8)
            .checked_add(length)
            .filter(|&n| n <= end)
            .ok_or(E::Win32(13))?;
        read(Chunk32 {
            id: &head[..4],
            data: &bytes[cursor + 8..finish],
            start: cursor + 8,
        })?;
        cursor = finish
            .checked_add(length & 1)
            .filter(|&n| n <= end)
            .ok_or(E::Win32(13))?;
    }
    Ok(())
}
impl AviClip32 {
    pub fn parse(bytes: Vec<u8>, limit: usize) -> Result<Self, E> {
        if bytes.len() > limit {
            return Err(E::Capacity);
        }
        if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"AVI ") {
            return Err(E::Win32(13));
        }
        let end = (word(&bytes, 4)? as usize)
            .checked_add(8)
            .filter(|&n| n == bytes.len())
            .ok_or(E::Win32(13))?;
        let mut header = None;
        let mut stream = None;
        let mut format = None;
        let mut movie = None;
        let mut streams = 0;
        chunks(&bytes, 12, end, |chunk| {
            if chunk.id != b"LIST" {
                return Ok(());
            }
            match chunk.data.get(..4) {
                Some(b"hdrl") => chunks(
                    &bytes,
                    chunk.start + 4,
                    chunk.start + chunk.data.len(),
                    |part| {
                        if part.id == b"avih" {
                            if header.is_some() || part.data.len() < 56 {
                                return Err(E::Win32(13));
                            }
                            if word(part.data, 12)? & 0x20 != 0 {
                                return Err(E::Win32(50));
                            }
                            header = Some((
                                word(part.data, 0)?,
                                word(part.data, 16)?,
                                word(part.data, 24)?,
                            ));
                        } else if part.id == b"LIST" && part.data.get(..4) == Some(b"strl") {
                            streams += 1;
                            chunks(
                                &bytes,
                                part.start + 4,
                                part.start + part.data.len(),
                                |entry| {
                                    if entry.id == b"strh" {
                                        if stream.is_some() || entry.data.len() < 56 {
                                            return Err(E::Win32(13));
                                        }
                                        if entry.data.get(..4) != Some(b"vids") {
                                            return Err(E::Win32(50));
                                        }
                                        let handler = &entry.data[4..8];
                                        if handler != b"DIB " && handler != [0, 0, 0, 0] {
                                            return Err(E::Win32(50));
                                        }
                                        stream = Some((
                                            word(entry.data, 20)?,
                                            word(entry.data, 24)?,
                                            word(entry.data, 32)?,
                                        ));
                                    } else if entry.id == b"strf" {
                                        if format.is_some() {
                                            return Err(E::Win32(13));
                                        }
                                        format = Some((entry.start, entry.data.len()));
                                    }
                                    Ok(())
                                },
                            )?;
                        }
                        Ok(())
                    },
                )?,
                Some(b"movi") => {
                    if movie.is_some() {
                        return Err(E::Win32(13));
                    }
                    movie = Some((chunk.start + 4, chunk.start + chunk.data.len()));
                }
                _ => {}
            }
            Ok(())
        })?;
        if streams != 1 {
            return Err(E::Win32(50));
        }
        let (micros, total, count) = header.ok_or(E::Win32(13))?;
        if count != 1 {
            return Err(E::Win32(50));
        }
        let (scale, rate, length) = stream.ok_or(E::Win32(13))?;
        if total == 0 || total > 65535 || length != total {
            return Err(E::Win32(13));
        }
        let (start, len) = format.ok_or(E::Win32(13))?;
        let info = &bytes[start..start + len];
        if info.len() < 40 || word(info, 0)? < 40 {
            return Err(E::Win32(50));
        }
        let mut words = [0; 10];
        for (i, value) in words.iter_mut().enumerate() {
            *value = word(info, i * 4)?;
        }
        let dib = decode_dib_header(words, || {
            Ok([
                word(info, 40).map_err(|_| crate::windows::gdi::GdiError32::InvalidRequest)?,
                word(info, 44).map_err(|_| crate::windows::gdi::GdiError32::InvalidRequest)?,
                word(info, 48).map_err(|_| crate::windows::gdi::GdiError32::InvalidRequest)?,
            ])
        })
        .map_err(|e| E::Win32(e.win32()))?;
        let bytes_per_frame = dib.layout.byte_len().map_err(|e| E::Win32(e.win32()))?;
        if bytes_per_frame > limit {
            return Err(E::Capacity);
        }
        let table = dib.table_offset as usize;
        let colors_bytes = dib
            .colors
            .checked_mul(4)
            .and_then(|n| table.checked_add(n))
            .filter(|&n| n <= info.len())
            .ok_or(E::Win32(13))?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(dib.colors)
            .map_err(|_| E::Allocation)?;
        colors.extend(
            info[table..colors_bytes]
                .chunks_exact(4)
                .map(|p| u32::from(p[2]) | u32::from(p[1]) << 8 | u32::from(p[0]) << 16),
        );
        let (first, last) = movie.ok_or(E::Win32(13))?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(total as usize)
            .map_err(|_| E::Allocation)?;
        let mut frame = |chunk: Chunk32<'_>| {
            if chunk.id == b"00db" || chunk.id == b"00dc" {
                if chunk.id == b"00dc" && words[4] != 0 && words[4] != 3 {
                    return Err(E::Win32(50));
                }
                if chunk.data.len() != bytes_per_frame || frames.len() == total as usize {
                    return Err(E::Win32(13));
                }
                frames.push(chunk.start..chunk.start + chunk.data.len());
            } else if chunk.id.get(2..) == Some(b"wb") || chunk.id.get(2..) == Some(b"pc") {
                return Err(E::Win32(50));
            }
            Ok(())
        };
        chunks(&bytes, first, last, |chunk| {
            if chunk.id == b"LIST" && chunk.data.get(..4) == Some(b"rec ") {
                chunks(
                    &bytes,
                    chunk.start + 4,
                    chunk.start + chunk.data.len(),
                    &mut frame,
                )
            } else {
                frame(chunk)
            }
        })?;
        if frames.len() != total as usize {
            return Err(E::Win32(13));
        }
        let period = if micros != 0 {
            u64::from(micros).div_ceil(1000)
        } else if scale != 0 && rate != 0 {
            (u64::from(scale) * 1000).div_ceil(u64::from(rate))
        } else {
            return Err(E::Win32(13));
        };
        Ok(Self {
            layout: dib.layout,
            colors,
            frames,
            period_ms: period.clamp(10, 0x7fff_ffff) as u32,
            bytes,
        })
    }
    pub fn color(&self, frame: usize, x: u32, y: u32) -> u32 {
        self.layout
            .read_pixel(&self.bytes[self.frames[frame].clone()], &self.colors, x, y)
    }
}
