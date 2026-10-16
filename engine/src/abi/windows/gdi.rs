use crate::{
    abi::{AbiError, header},
    process::GdiFrame32,
};
pub const GDI_FRAME_SIZE: usize = 80;
pub fn encode_gdi_frame(frame: Option<GdiFrame32>, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != GDI_FRAME_SIZE {
        return Err(AbiError::Length);
    }
    header::write(output, *b"R3GD");
    if let Some(frame) = frame {
        header::write_u32(output, 12, 1);
        output[16..24].copy_from_slice(&frame.id.to_le_bytes());
        output[24..32].copy_from_slice(&frame.revision.to_le_bytes());
        for (index, word) in [
            frame.window,
            u32::from(frame.window_dc),
            frame.width,
            frame.height,
            frame.stride,
            frame.bytes,
            frame.damage.left as u32,
            frame.damage.top as u32,
            frame.damage.right as u32,
            frame.damage.bottom as u32,
            frame.format,
        ]
        .into_iter()
        .enumerate()
        {
            header::write_u32(output, 32 + index * 4, word);
        }
    }
    Ok(())
}
