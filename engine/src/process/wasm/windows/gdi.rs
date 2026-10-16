use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        windows::gdi::{GDI_FRAME_SIZE, encode_gdi_frame},
    },
    process::{GdiConfig32, HostError, wasm::mutate},
};
pub(crate) fn configure_gdi(
    objects: u32,
    dcs: u32,
    saved_states: u32,
    bitmap_bytes: u32,
    bitmap_base: u32,
    bitmap_pages: u32,
    screen_width: u32,
    screen_height: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_gdi(GdiConfig32 {
            objects: objects as usize,
            dcs: dcs as usize,
            saved_states: saved_states as usize,
            bitmap_bytes: bitmap_bytes as usize,
            bitmap_base,
            bitmap_pages,
            screen_width,
            screen_height,
        })
    })
}
pub(crate) fn register_gdi32_module(base: u32) -> u32 {
    mutate(|instance| instance.register_gdi32_module(base))
}
pub(crate) fn pending_gdi_frame(index: u32) -> u32 {
    mutate(|instance| {
        let frame = instance.pending_gdi_frame(index)?;
        let mut bytes = [0; GDI_FRAME_SIZE];
        encode_gdi_frame(frame, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + GDI_FRAME_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    })
}
pub(crate) fn read_gdi_frame(
    id_low: u32,
    id_high: u32,
    revision_low: u32,
    revision_high: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; 4096];
        let input = instance.read_gdi_frame(
            super::wide(id_low, id_high),
            super::wide(revision_low, revision_high),
            offset,
            length,
        )?;
        let count = input.len();
        bytes[..count].copy_from_slice(input);
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + count]
            .copy_from_slice(&bytes[..count]);
        Ok(())
    })
}
pub(crate) fn acknowledge_gdi_frame(
    id_low: u32,
    id_high: u32,
    revision_low: u32,
    revision_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.acknowledge_gdi_frame(
            super::wide(id_low, id_high),
            super::wide(revision_low, revision_high),
        )
    })
}
