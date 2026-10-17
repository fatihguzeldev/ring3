use super::CapturedFileDialog32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        commdlg::{ComdlgError32 as E, FileSelection32},
        text::CodePage32,
    },
};

impl CapturedFileDialog32 {
    pub(in crate::process::windows) fn publish(
        &self,
        memory: &mut AddressSpace,
        path: &[u16],
        filter: u32,
        readonly: bool,
    ) -> Result<u32, E> {
        self.outputs.check(memory)?;
        let selected = FileSelection32::single(path, self.wide, self.ansi, 1 << 20)?;
        let too_small = selected.units > self.input.file_capacity;
        let required = u16::try_from(selected.units)
            .map_err(|_| E::Capacity)?
            .to_le_bytes();
        let mut info = [0; 12];
        info[..4].copy_from_slice(&filter.to_le_bytes());
        let flags = (self.input.flags & !1) | u32::from(readonly);
        info[4..8].copy_from_slice(&flags.to_le_bytes());
        info[8..10].copy_from_slice(&selected.file_offset.to_le_bytes());
        info[10..12].copy_from_slice(&selected.extension_offset.to_le_bytes());
        let mut title = Vec::new();
        if self.input.file_title != 0 && self.input.title_capacity > 0 && !too_small {
            let at = path
                .iter()
                .rposition(|&unit| unit == 92)
                .map_or(0, |at| at + 1);
            title = encode(&path[at..], self.wide, self.ansi)?;
            let width = if self.wide { 2 } else { 1 };
            if title.len() / width > self.input.title_capacity as usize {
                title.clear();
            }
        }
        let mut custom = Vec::new();
        if let Some(custom_filter) = &self.filters.custom
            && let Some(current) = self.filters.selected(filter)
        {
            let mut units = custom_filter.label.clone();
            units.push(0);
            units.extend_from_slice(&current.pattern);
            custom = encode(&units, self.wide, self.ansi)?;
            if custom.len() / if self.wide { 2 } else { 1 } > self.input.custom_capacity as usize {
                custom.clear();
            }
        }
        let mut writes = Vec::new();
        writes.try_reserve_exact(6).map_err(|_| E::Allocation)?;
        if too_small {
            // native wow32 reports the required word even though its BOOL is true.
            if self.input.file_capacity as usize * if self.wide { 2 } else { 1 } >= 2 {
                writes.push((GuestAddress(self.input.file), required.as_slice()));
            }
            if self.wide {
                writes.push((
                    GuestAddress(self.pointer.checked_add(56).ok_or(E::InvalidRequest)?),
                    &info[8..12],
                ));
            }
        } else {
            writes.push((GuestAddress(self.input.file), selected.bytes.as_slice()));
            writes.push((
                GuestAddress(self.pointer.checked_add(56).ok_or(E::InvalidRequest)?),
                &info[8..12],
            ));
            if !title.is_empty() {
                writes.push((GuestAddress(self.input.file_title), title.as_slice()));
            }
            if !custom.is_empty() {
                writes.push((GuestAddress(self.input.custom_filter), custom.as_slice()));
            }
        }
        writes.push((
            GuestAddress(self.pointer.checked_add(24).ok_or(E::InvalidRequest)?),
            &info[..4],
        ));
        writes.push((
            GuestAddress(self.pointer.checked_add(52).ok_or(E::InvalidRequest)?),
            &info[4..8],
        ));
        memory.write_scatter(&writes)?;
        Ok(if too_small { 0x3003 } else { 0 })
    }
}

pub(in crate::process::windows) fn encode(
    units: &[u16],
    wide: bool,
    ansi: CodePage32,
) -> Result<Vec<u8>, E> {
    let mut bytes = if wide {
        units
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>()
    } else {
        ansi.encode(units, false).map_err(|_| E::InvalidRequest)?.0
    };
    bytes
        .try_reserve(if wide { 2 } else { 1 })
        .map_err(|_| E::Allocation)?;
    bytes.push(0);
    if wide {
        bytes.push(0);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};
    fn request(memory: &mut AddressSpace, wide: bool) -> CapturedFileDialog32 {
        let range = PageRange::new(GuestAddress(0x10000), 2).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        let mut bytes = [0; 88];
        for (offset, value) in [(0, 88u32), (28, 0x11000), (32, 2), (56, 0x00090007)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        memory.write(GuestAddress(0x10000), &bytes).unwrap();
        CapturedFileDialog32::read(memory, 0x10000, wide, CodePage32::Windows1252, 4096).unwrap()
    }
    #[test]
    fn native_wow32_small_buffer_receipt_keeps_wide_offsets_and_ansi_original_offsets() {
        for wide in [false, true] {
            let mut memory = AddressSpace::new(4).unwrap();
            let request = request(&mut memory, wide);
            assert_eq!(
                request.publish(
                    &mut memory,
                    &r"C:\file.txt".encode_utf16().collect::<Vec<_>>(),
                    1,
                    false
                ),
                Ok(0x3003)
            );
            let mut required = [0; 2];
            memory.read(GuestAddress(0x11000), &mut required).unwrap();
            assert_eq!(u16::from_le_bytes(required), 12);
            let mut offsets = [0; 4];
            memory.read(GuestAddress(0x10038), &mut offsets).unwrap();
            assert_eq!(offsets, if wide { [3, 0, 8, 0] } else { [7, 0, 9, 0] });
        }
    }
}
