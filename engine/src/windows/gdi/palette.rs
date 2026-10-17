use super::{GdiContext32, GdiError32, GdiObject32};

pub const DEFAULT_PALETTE_ENTRIES32: [u32; 20] = [
    0, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0xc0c0c0, 0xc0dcc0, 0xf0caa6,
    0xf0fbff, 0xa4a0a0, 0x808080, 0x0000ff, 0x00ff00, 0x00ffff, 0xff0000, 0xff00ff, 0xffff00,
    0xffffff,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalPalette32 {
    entries: Vec<u32>,
}
impl LogicalPalette32 {
    pub fn new(entries: Vec<u32>) -> Result<Self, GdiError32> {
        if entries.is_empty() || entries.len() > u16::MAX as usize {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(Self { entries })
    }
    pub fn entries(&self) -> &[u32] {
        &self.entries
    }
    pub fn set_entries(&mut self, start: usize, entries: &[u32]) -> usize {
        let count = entries.len().min(self.entries.len().saturating_sub(start));
        if count != 0 {
            self.entries[start..start + count].copy_from_slice(&entries[..count]);
        }
        count
    }
    pub fn resize(&mut self, count: usize) -> Result<(), GdiError32> {
        if !(1..=1024).contains(&count) {
            return Err(GdiError32::InvalidRequest);
        }
        if count > self.entries.len() {
            self.entries
                .try_reserve_exact(count - self.entries.len())
                .map_err(|_| GdiError32::Allocation)?;
        }
        self.entries.resize(count, 0);
        Ok(())
    }
}

pub fn nearest_palette_index32(entries: &[u32], color: u32) -> Option<usize> {
    let mut best = None;
    let mut distance = u32::MAX;
    for (index, candidate) in entries.iter().enumerate() {
        let difference = [0, 8, 16]
            .into_iter()
            .map(|shift| {
                let delta = ((candidate >> shift) & 255) as i32 - ((color >> shift) & 255) as i32;
                (delta * delta) as u32
            })
            .sum();
        if difference < distance {
            best = Some(index);
            distance = difference;
        }
        if distance == 0 {
            break;
        }
    }
    best
}

impl GdiContext32 {
    pub fn palette_entries(&self, handle: u32) -> Result<&[u32], GdiError32> {
        if handle == super::context::stock_handle(15) {
            return Ok(&DEFAULT_PALETTE_ENTRIES32);
        }
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        match &self.objects[index].object {
            GdiObject32::Palette(palette) => Ok(palette.entries()),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub fn palette_mut(&mut self, handle: u32) -> Result<&mut LogicalPalette32, GdiError32> {
        self.check_write_object(handle)?;
        let index = self
            .objects
            .binary_search_by_key(&handle, |entry| entry.handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        match &mut self.objects[index].object {
            GdiObject32::Palette(palette) => Ok(palette),
            _ => Err(GdiError32::InvalidHandle),
        }
    }
    pub fn palette_bytes(&self) -> usize {
        self.objects
            .iter()
            .map(|entry| match &entry.object {
                GdiObject32::Palette(palette) => palette.entries.len() * 4,
                _ => 0,
            })
            .sum()
    }
    pub fn select_palette(&mut self, dc: u32, palette: u32) -> Result<u32, GdiError32> {
        self.palette_entries(palette)?;
        let mut state = self.state(dc)?.clone();
        let previous = state.palette;
        state.palette = palette;
        self.set_state(dc, state)?;
        Ok(previous)
    }
}

pub fn synthesized_palette32(bits: u16) -> Result<Vec<u32>, GdiError32> {
    let count = match bits {
        1 => 2,
        4 => 16,
        8 => 256,
        _ => 0,
    };
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| GdiError32::Allocation)?;
    match bits {
        1 => entries.extend([0, 0xffffff]),
        4 => entries.extend([
            0, 0x000080, 0x008000, 0x008080, 0x800000, 0x800080, 0x808000, 0x808080, 0xc0c0c0,
            0x0000ff, 0x00ff00, 0x00ffff, 0xff0000, 0xff00ff, 0xffff00, 0xffffff,
        ]),
        8 => {
            for index in 0..256 {
                entries.push(
                    (index & 7) * 32 | ((index >> 3) & 7) * 32 << 8 | ((index >> 6) & 3) * 64 << 16,
                );
            }
            entries[..10].copy_from_slice(&DEFAULT_PALETTE_ENTRIES32[..10]);
            entries[246..].copy_from_slice(&DEFAULT_PALETTE_ENTRIES32[10..]);
        }
        _ => {}
    }
    Ok(entries)
}
