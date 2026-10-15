use super::*;
use crate::{
    memory::{GuestAddress, PageRange},
    process::{FileConfig32, VfsLimits32},
};

pub fn decode_file_config(bytes: &[u8]) -> Result<FileConfig32, AbiError> {
    header(bytes, *b"R3FC", 80)?;
    if bytes[72..].iter().any(|byte| *byte != 0) {
        return Err(AbiError::Reserved);
    }
    let values: [u32; 14] = std::array::from_fn(|i| word(bytes, 16 + i * 4));
    Ok(FileConfig32 {
        vfs: VfsLimits32 {
            nodes: values[0] as usize,
            opens: values[1] as usize,
            cache_pages: values[2] as usize,
            overlay_pages: values[3] as usize,
            requests: values[4] as usize,
            max_io_bytes: values[5] as usize,
        },
        handles: values[6],
        finds: values[7] as usize,
        mappings: values[8] as usize,
        views: values[9] as usize,
        operations: values[10] as usize,
        operation_bytes: values[11] as usize,
        view_bounds: PageRange::new(GuestAddress(values[12]), values[13])
            .map_err(|_| AbiError::Flags)?,
    })
}
pub fn encode_file_config(config: FileConfig32, bytes: &mut [u8]) -> Result<(), AbiError> {
    if bytes.len() != 80 {
        return Err(AbiError::Length);
    }
    let raw = [
        config.vfs.nodes,
        config.vfs.opens,
        config.vfs.cache_pages,
        config.vfs.overlay_pages,
        config.vfs.requests,
        config.vfs.max_io_bytes,
        config.handles as usize,
        config.finds,
        config.mappings,
        config.views,
        config.operations,
        config.operation_bytes,
        config.view_bounds.first * 4096,
        config.view_bounds.count,
    ];
    let mut values = [0u32; 14];
    for (i, value) in raw.iter().enumerate() {
        values[i] = u32::try_from(*value).map_err(|_| AbiError::Flags)?;
    }
    initialize(bytes, *b"R3FC");
    for (i, value) in values.iter().enumerate() {
        put(bytes, 16 + i * 4, *value);
    }
    Ok(())
}
