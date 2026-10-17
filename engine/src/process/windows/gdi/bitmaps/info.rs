use super::super::{context::GdiRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::gdi::{BitmapLayout32, GdiError32},
};

pub(in crate::process::windows::gdi) fn dib_layout(
    memory: &AddressSpace,
    address: u32,
    header: [u32; 10],
) -> Result<crate::windows::gdi::DibHeader32, GdiError32> {
    crate::windows::gdi::decode_dib_header(header, || {
        memory::words(
            memory,
            address.checked_add(40).ok_or(GdiError32::InvalidRequest)?,
        )
    })
}
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn read_dib_info(
        &self,
        memory: &AddressSpace,
        address: u32,
        usage: u32,
        dc: u32,
    ) -> Result<(BitmapLayout32, Vec<u32>), GdiError32> {
        if usage > 1 {
            return Err(GdiError32::InvalidRequest);
        }
        let info = dib_layout(memory, address, memory::words(memory, address)?)?;
        if usage == 1 && info.layout.masks.is_some() {
            return Err(GdiError32::InvalidRequest);
        }
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(info.colors)
            .map_err(|_| GdiError32::Allocation)?;
        if info.colors != 0 {
            let size = if usage == 1 { 2 } else { 4 };
            let mut bytes = memory::zeroes(info.colors * size)?;
            memory.read(
                GuestAddress(
                    address
                        .checked_add(info.table_offset)
                        .ok_or(GdiError32::InvalidRequest)?,
                ),
                &mut bytes,
            )?;
            if usage == 0 {
                colors.extend(bytes.chunks_exact(4).map(quad_to_color));
            } else {
                let entries = self.selected_palette(dc)?;
                if entries.is_empty() {
                    return Err(GdiError32::InvalidRequest);
                }
                colors.extend(bytes.chunks_exact(2).map(|part| {
                    let index = u16::from_le_bytes(part.try_into().unwrap()) as usize;
                    entries[index % entries.len()] & 0xffffff
                }));
            }
        }
        Ok((info.layout, colors))
    }
}

pub(in crate::process::windows::gdi) fn quad_to_color(bytes: &[u8]) -> u32 {
    u32::from(bytes[2]) | u32::from(bytes[1]) << 8 | u32::from(bytes[0]) << 16
}
pub(in crate::process::windows::gdi) fn color_to_quad(color: u32) -> [u8; 4] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8, 0]
}
