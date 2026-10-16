use super::{context::GdiRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::gdi::{GdiApi32, GdiError32, GdiObject32, LogicalPalette32, nearest_palette_index32},
};

impl GdiRuntime32 {
    fn check_palette_bytes(&self, old: usize, new: usize) -> Result<(), GdiError32> {
        let total = self
            .model
            .palette_bytes()
            .checked_sub(old)
            .and_then(|bytes| bytes.checked_add(new))
            .and_then(|bytes| bytes.checked_add(self.bytes));
        if total.is_none_or(|bytes| bytes > self.config.bitmap_bytes) {
            Err(GdiError32::Capacity)
        } else {
            Ok(())
        }
    }

    pub(super) fn palette_call(
        &mut self,
        ram: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        use GdiApi32::*;
        match api {
            CreatePalette => {
                let [header] = memory::words::<1>(ram, args[0])?;
                if header & 0xffff != 0x300 {
                    return Err(GdiError32::InvalidRequest);
                }
                let count = (header >> 16) as usize;
                if count == 0 {
                    return Ok(0);
                }
                self.check_palette_bytes(0, count * 4)?;
                let mut bytes = memory::zeroes(count * 4)?;
                ram.read(
                    GuestAddress(args[0].checked_add(4).ok_or(GdiError32::InvalidRequest)?),
                    &mut bytes,
                )?;
                let mut entries = Vec::new();
                entries
                    .try_reserve_exact(count)
                    .map_err(|_| GdiError32::Allocation)?;
                entries.extend(
                    bytes
                        .chunks_exact(4)
                        .map(|part| u32::from_le_bytes(part.try_into().unwrap())),
                );
                let prepared = self
                    .model
                    .prepare_object(GdiObject32::Palette(LogicalPalette32::new(entries)?))?;
                Ok(self.model.commit_new(prepared))
            }
            SelectPalette => self.model.select_palette(args[0], args[1]),
            GetPaletteEntries | SetPaletteEntries => {
                let entries = self.model.palette_entries(args[0])?;
                if api == GetPaletteEntries && (args[3] == 0 || args[2] == 0) {
                    return Ok(entries.len() as u32);
                }
                if api == SetPaletteEntries && self.model.is_stock(args[0]) {
                    return Ok(0);
                }
                let start = args[1] as usize;
                let count = (args[2] as usize).min(entries.len().saturating_sub(start));
                if count == 0 {
                    return Ok(0);
                }
                let mut bytes = memory::zeroes(count * 4)?;
                if api == GetPaletteEntries {
                    for (part, entry) in bytes
                        .chunks_exact_mut(4)
                        .zip(&entries[start..start + count])
                    {
                        part.copy_from_slice(&entry.to_le_bytes());
                    }
                    ram.write(GuestAddress(args[3]), &bytes)?;
                } else {
                    ram.read(GuestAddress(args[3]), &mut bytes)?;
                    let palette = self.model.palette_mut(args[0])?;
                    for (index, part) in bytes.chunks_exact(4).enumerate() {
                        palette.set_entries(
                            start + index,
                            &[u32::from_le_bytes(part.try_into().unwrap())],
                        );
                    }
                }
                Ok(count as u32)
            }
            ResizePalette => {
                let old = self.model.palette_entries(args[0])?.len() * 4;
                if !(1..=1024).contains(&args[1]) {
                    return Err(GdiError32::InvalidRequest);
                }
                self.check_palette_bytes(old, args[1] as usize * 4)?;
                self.model.palette_mut(args[0])?.resize(args[1] as usize)?;
                Ok(1)
            }
            GetNearestPaletteIndex => {
                nearest_palette_index32(self.model.palette_entries(args[0])?, args[1])
                    .map(|value| value as u32)
                    .ok_or(GdiError32::InvalidRequest)
            }
            RealizePalette => {
                let palette = self.model.state(args[0])?.palette;
                self.model.palette_entries(palette)?;
                // this configured display is true-color and has no physical palette to realize.
                Ok(0)
            }
            GetSystemPaletteEntries => {
                self.model.dc_kind(args[0])?;
                Ok(0)
            }
            _ => Err(GdiError32::InvalidRequest),
        }
    }
    pub(in crate::process::windows::gdi) fn selected_palette(
        &self,
        dc: u32,
    ) -> Result<&[u32], GdiError32> {
        let palette = if dc == 0 {
            crate::windows::gdi::GdiContext32::stock_object(15)?
        } else {
            self.model.state(dc)?.palette
        };
        self.model.palette_entries(palette)
    }
}
