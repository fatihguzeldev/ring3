use super::super::super::memory::{read, words};
use super::{MAX_ENTRIES, MAX_FILE_BYTES};
use crate::{
    input::dinput::{
        DirectInputResult32 as H,
        haptics::{
            EffectEnvelope32, EffectKind32,
            files::{FileEffect32, encode},
        },
    },
    memory::{Access, AddressSpace, GuestAddress},
};

pub(super) fn filename(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    code_page: crate::windows::text::CodePage32,
) -> Result<Vec<u16>, H> {
    let mut result = Vec::new();
    let mut ansi = Vec::new();
    if wide {
        result.try_reserve_exact(32767)
    } else {
        ansi.try_reserve_exact(32767)
    }
    .map_err(|_| H::OUT_OF_MEMORY)?;
    for index in 0..32767u32 {
        let at = address
            .checked_add(index * if wide { 2 } else { 1 })
            .ok_or(H::INVALID_PARAM)?;
        let value = if wide {
            u16::from_le_bytes(read(memory, at)?)
        } else {
            u16::from(read::<1>(memory, at)?[0])
        };
        if value == 0 {
            if wide {
                return if result.is_empty() {
                    Err(H::INVALID_PARAM)
                } else {
                    Ok(result)
                };
            }
            if ansi.is_empty() {
                return Err(H::INVALID_PARAM);
            }
            return code_page.decode(&ansi, false).map_err(|_| H::INVALID_PARAM);
        }
        if wide {
            result.push(value)
        } else {
            ansi.push(value as u8)
        }
    }
    Err(H::INVALID_PARAM)
}
pub(super) fn snapshot(
    memory: &AddressSpace,
    address: u32,
    count: u32,
    flags: u32,
) -> Result<Vec<u8>, H> {
    if flags & !1 != 0 || count as usize > MAX_ENTRIES {
        return Err(H::INVALID_PARAM);
    }
    let length = (count as usize).checked_mul(284).ok_or(H::INVALID_PARAM)?;
    memory.check_span(GuestAddress(address), length, Access::Read)?;
    let mut effects = Vec::new();
    effects
        .try_reserve_exact(count as usize)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    for index in 0..count {
        let at = address.checked_add(index * 284).ok_or(H::INVALID_PARAM)?;
        let entry = read::<284>(memory, at)?;
        if u32::from_le_bytes(entry[..4].try_into().unwrap()) != 284 {
            return Err(H::INVALID_PARAM);
        }
        let guid = entry[4..20].try_into().unwrap();
        if flags & 1 == 0 && EffectKind32::from_guid(guid).is_none() {
            continue;
        }
        let end = entry[24..]
            .iter()
            .position(|value| *value == 0)
            .ok_or(H::INVALID_PARAM)?;
        let name = owned(&entry[24..24 + end])?;
        let pointer = u32::from_le_bytes(entry[20..24].try_into().unwrap());
        let size = words::<1>(memory, pointer)?[0];
        if !matches!(size, 52 | 56) {
            return Err(H::INVALID_PARAM);
        }
        let mut header = [0u32; 14];
        for (index, value) in header[..size as usize / 4].iter_mut().enumerate() {
            *value = words::<1>(
                memory,
                pointer
                    .checked_add(index as u32 * 4)
                    .ok_or(H::INVALID_PARAM)?,
            )?[0]
        }
        if header[7] > 32 || header[11] > 131072 {
            return Err(H::INVALID_PARAM);
        }
        let axis = read_owned(memory, header[8], header[7] as usize * 4)?;
        let direction = read_owned(memory, header[9], header[7] as usize * 4)?;
        let mut axes = Vec::new();
        let mut directions = Vec::new();
        axes.try_reserve_exact(header[7] as usize)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        directions
            .try_reserve_exact(header[7] as usize)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        axes.extend(
            axis.chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())),
        );
        directions.extend(
            direction
                .chunks_exact(4)
                .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
        );
        let envelope = if header[10] == 0 {
            None
        } else {
            let values = words::<5>(memory, header[10])?;
            if values[0] != 20 {
                return Err(H::INVALID_PARAM);
            }
            Some(EffectEnvelope32 {
                attack_level: values[1],
                attack_time: values[2],
                fade_level: values[3],
                fade_time: values[4],
            })
        };
        let type_specific = read_owned(memory, header[12], header[11] as usize)?;
        effects.push(FileEffect32 {
            guid,
            name,
            flags: header[1],
            duration: header[2],
            sample_period: header[3],
            gain: header[4],
            trigger: header[5],
            trigger_repeat: header[6],
            axes,
            direction: directions,
            envelope,
            type_specific,
            opaque_chunks: Vec::new(),
            start_delay: header[13],
        });
    }
    encode(&effects, MAX_FILE_BYTES)
}
fn read_owned(memory: &AddressSpace, address: u32, length: usize) -> Result<Vec<u8>, H> {
    if length == 0 {
        return Ok(Vec::new());
    }
    memory.check_span(GuestAddress(address), length, Access::Read)?;
    let mut bytes = super::super::super::memory::bytes(length)?;
    memory.read(GuestAddress(address), &mut bytes)?;
    Ok(bytes)
}
fn owned(bytes: &[u8]) -> Result<Vec<u8>, H> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes.len())
        .map_err(|_| H::OUT_OF_MEMORY)?;
    result.extend_from_slice(bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::{PageRange, Permissions},
        windows::text::CodePage32,
    };
    #[test]
    fn file_name_decoding_uses_the_configured_ansi_code_page() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1000), b"C:\\caf\xc3\xa9.ffe\0")
            .unwrap();
        assert_eq!(
            filename(&memory, 0x1000, false, CodePage32::Utf8).unwrap(),
            "C:\\caf\u{e9}.ffe".encode_utf16().collect::<Vec<_>>()
        );
        memory
            .write(GuestAddress(0x1000), b"C:\\\x80.ffe\0")
            .unwrap();
        assert_eq!(
            filename(&memory, 0x1000, false, CodePage32::Windows1252).unwrap(),
            "C:\\\u{20ac}.ffe".encode_utf16().collect::<Vec<_>>()
        );
    }
    #[test]
    fn file_snapshot_preflights_the_complete_array_before_allocating_or_reading_headers() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0xfffff000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        assert_eq!(snapshot(&memory, 0xfffffff0, 1, 0), Err(H::INVALID_PARAM));
        assert_eq!(snapshot(&memory, 0xfffff000, 257, 0), Err(H::INVALID_PARAM));
    }

    #[test]
    fn custom_file_snapshot_copies_only_raw_metadata_even_when_sample_pointer_is_unmapped() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        let mut entry = [0; 284];
        entry[..4].copy_from_slice(&284u32.to_le_bytes());
        entry[4..20].copy_from_slice(&EffectKind32::Custom.guid());
        entry[20..24].copy_from_slice(&0x1300u32.to_le_bytes());
        memory.write(GuestAddress(0x1100), &entry).unwrap();
        let header = [
            56u32,
            0x11,
            10000,
            0,
            10000,
            u32::MAX,
            0,
            2,
            0x1400,
            0x1410,
            0,
            16,
            0x1420,
            0,
        ];
        memory
            .write(GuestAddress(0x1300), &header.map(u32::to_le_bytes).concat())
            .unwrap();
        memory
            .write(
                GuestAddress(0x1400),
                &[2u32, 0x102].map(u32::to_le_bytes).concat(),
            )
            .unwrap();
        memory
            .write(
                GuestAddress(0x1410),
                &[1u32, 0].map(u32::to_le_bytes).concat(),
            )
            .unwrap();
        let raw = [2u32, 1000, 8, 0x7000_0000].map(u32::to_le_bytes).concat();
        memory.write(GuestAddress(0x1420), &raw).unwrap();
        let version = memory.transaction_version();
        let bytes = snapshot(&memory, 0x1100, 1, 0).unwrap();
        assert_eq!(bytes.len(), 236);
        assert_eq!(&bytes[220..], raw);
        assert_eq!(memory.transaction_version(), version);
        assert!(
            memory
                .mapping_version_at(GuestAddress(0x7000_0000))
                .is_err()
        );
    }
}
