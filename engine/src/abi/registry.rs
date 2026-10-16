use crate::{abi::header, windows::registry::RegistryChange32};

pub const CHANGE_SIZE: usize = 128;
pub fn encode_change(change: Option<RegistryChange32<'_>>, out: &mut [u8; CHANGE_SIZE]) {
    header::write(out, *b"R3RG");
    if let Some(change) = change {
        header::write_u32(out, 12, 1);
        for (at, value) in [
            (16, change.id.epoch),
            (24, change.id.sequence),
            (32, change.id.revision),
            (64, change.modified),
        ] {
            out[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        for (at, value) in [
            (40, change.kind as u32),
            (44, change.root),
            (48, change.view as u32),
            (52, change.path.len() as u32),
            (56, change.name.len() as u32),
            (60, change.class.len() as u32),
            (72, change.value.map_or(0, |value| value.kind)),
            (76, change.value.map_or(0, |value| value.bytes.len() as u32)),
            (80, u32::from(change.value.is_some())),
        ] {
            header::write_u32(out, at, value);
        }
    }
}
