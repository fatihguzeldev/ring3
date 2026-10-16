use super::GdiError32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitmapLayout32 {
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u16,
    pub stride: u32,
    pub top_down: bool,
    pub dib: bool,
    pub masks: Option<[u32; 3]>,
}
pub type BitmapPalette32 = Vec<u32>;
impl BitmapLayout32 {
    pub fn new(width: i32, height: i32, bits: u16, dib: bool) -> Result<Self, GdiError32> {
        if width <= 0
            || height == 0
            || height == i32::MIN
            || !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32)
        {
            return Err(GdiError32::InvalidRequest);
        }
        let width = width as u32;
        let row_bits = u64::from(width) * u64::from(bits);
        let alignment = if dib { 32 } else { 16 };
        let stride = u32::try_from(row_bits.div_ceil(alignment) * (alignment / 8))
            .map_err(|_| GdiError32::Capacity)?;
        let result = Self {
            width,
            height: height.unsigned_abs(),
            bits_per_pixel: bits,
            stride,
            top_down: !dib || height < 0,
            dib,
            masks: None,
        };
        result.byte_len()?;
        Ok(result)
    }
    pub fn with_masks(mut self, masks: [u32; 3]) -> Result<Self, GdiError32> {
        if !matches!(self.bits_per_pixel, 16 | 32) {
            return Err(GdiError32::InvalidRequest);
        }
        let allowed = u32::MAX >> (32 - self.bits_per_pixel);
        let mut used = 0;
        for mask in masks {
            if mask == 0 {
                return Err(GdiError32::InvalidRequest);
            }
            let normalized = mask >> mask.trailing_zeros();
            if mask & !allowed != 0
                || mask & used != 0
                || normalized & normalized.wrapping_add(1) != 0
            {
                return Err(GdiError32::InvalidRequest);
            }
            used |= mask;
        }
        self.masks = Some(masks);
        Ok(self)
    }
    pub fn compression(self) -> u32 {
        if self.masks.is_some() { 3 } else { 0 }
    }
    pub fn byte_len(self) -> Result<usize, GdiError32> {
        usize::try_from(u64::from(self.stride) * u64::from(self.height))
            .map_err(|_| GdiError32::Capacity)
    }
    pub fn row(self, y: u32) -> usize {
        (if self.top_down {
            y as usize
        } else {
            (self.height - y - 1) as usize
        }) * self.stride as usize
    }
    pub(crate) fn pixel_span(self, x: u32, y: u32) -> Result<(usize, usize, u32), GdiError32> {
        if x >= self.width || y >= self.height {
            return Err(GdiError32::InvalidRequest);
        }
        let (byte, count, local_x) = match self.bits_per_pixel {
            1 => (x as usize / 8, 1, x % 8),
            4 => (x as usize / 2, 1, x % 2),
            8 => (x as usize, 1, 0),
            bits @ (16 | 24 | 32) => (x as usize * usize::from(bits) / 8, usize::from(bits) / 8, 0),
            _ => return Err(GdiError32::InvalidRequest),
        };
        Ok((self.row(y) + byte, count, local_x))
    }
    pub fn read_pixel(self, bytes: &[u8], palette: &[u32], x: u32, y: u32) -> u32 {
        let row = self.row(y);
        if let Some(masks) = self.masks {
            let at = row + x as usize * usize::from(self.bits_per_pixel) / 8;
            let value = if self.bits_per_pixel == 16 {
                u32::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
            } else {
                u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
            };
            return masks
                .into_iter()
                .enumerate()
                .fold(0, |color, (channel, mask)| {
                    let component = (value & mask) >> mask.trailing_zeros();
                    color | resize_component(component, mask.count_ones(), 8) << (channel * 8)
                });
        }
        match self.bits_per_pixel {
            1 => palette
                .get(((bytes[row + x as usize / 8] >> (7 - x % 8)) & 1) as usize)
                .copied()
                .unwrap_or(0),
            4 => palette
                .get(
                    ((bytes[row + x as usize / 2] >> (if x % 2 == 0 { 4 } else { 0 })) & 15)
                        as usize,
                )
                .copied()
                .unwrap_or(0),
            8 => palette
                .get(bytes[row + x as usize] as usize)
                .copied()
                .unwrap_or(0),
            16 => {
                let at = row + x as usize * 2;
                let value = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
                let expand = |value: u16| u32::from((value << 3) | (value >> 2));
                expand((value >> 10) & 31)
                    | (expand((value >> 5) & 31) << 8)
                    | (expand(value & 31) << 16)
            }
            24 | 32 => {
                let at = row + x as usize * (self.bits_per_pixel as usize / 8);
                u32::from(bytes[at + 2])
                    | (u32::from(bytes[at + 1]) << 8)
                    | (u32::from(bytes[at]) << 16)
            }
            _ => unreachable!(),
        }
    }
    pub fn write_pixel(self, bytes: &mut [u8], palette: &[u32], x: u32, y: u32, color: u32) {
        let row = self.row(y);
        if let Some(masks) = self.masks {
            let at = row + x as usize * usize::from(self.bits_per_pixel) / 8;
            let value = masks
                .into_iter()
                .enumerate()
                .fold(0, |value, (channel, mask)| {
                    let component = (color >> (channel * 8)) & 255;
                    value
                        | resize_component(component, 8, mask.count_ones()) << mask.trailing_zeros()
                });
            let count = usize::from(self.bits_per_pixel) / 8;
            bytes[at..at + count].copy_from_slice(&value.to_le_bytes()[..count]);
            return;
        }
        let index = || {
            palette
                .iter()
                .enumerate()
                .min_by_key(|(_, candidate)| {
                    [0, 8, 16]
                        .iter()
                        .map(|shift| {
                            let delta = ((**candidate >> shift) & 255) as i32
                                - ((color >> shift) & 255) as i32;
                            delta * delta
                        })
                        .sum::<i32>()
                })
                .map_or(0, |(index, _)| index as u8)
        };
        match self.bits_per_pixel {
            1 => {
                let bit = 7 - x % 8;
                let value = &mut bytes[row + x as usize / 8];
                *value = (*value & !(1 << bit)) | ((index() & 1) << bit);
            }
            4 => {
                let shift = if x % 2 == 0 { 4 } else { 0 };
                let value = &mut bytes[row + x as usize / 2];
                *value = (*value & !(15 << shift)) | ((index() & 15) << shift);
            }
            8 => bytes[row + x as usize] = index(),
            16 => {
                let value =
                    ((color >> 3) & 31) << 10 | ((color >> 11) & 31) << 5 | ((color >> 19) & 31);
                let at = row + x as usize * 2;
                bytes[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
            }
            24 | 32 => {
                let at = row + x as usize * (self.bits_per_pixel as usize / 8);
                bytes[at..at + 3].copy_from_slice(&[
                    (color >> 16) as u8,
                    (color >> 8) as u8,
                    color as u8,
                ]);
            }
            _ => unreachable!(),
        }
    }
}

fn resize_component(value: u32, source_bits: u32, target_bits: u32) -> u32 {
    if source_bits >= target_bits {
        return value >> (source_bits - target_bits);
    }
    let mut output = value;
    let mut count = source_bits;
    while count < target_bits {
        let next = source_bits.min(target_bits - count);
        output = (output << next) | (value >> (source_bits - next));
        count += next;
    }
    output
}
