use super::{Access, DispatchError, GuestMemory, INVALID_ARGUMENT, NULL_POINTER, guest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    StandardMouse2,
}

pub(super) struct Device {
    pub(super) references: u32,
    format: Option<Format>,
}

impl Device {
    pub(super) fn new() -> Self {
        Self {
            references: 1,
            format: None,
        }
    }

    pub(super) fn set_format(
        &mut self,
        address: u32,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        if address == 0 {
            return Ok(NULL_POINTER);
        }
        let mut header = [0; 6];
        guest::read_words(memory, address, &mut header[..1])?;
        if header[0] != 24 {
            return Ok(INVALID_ARGUMENT);
        }
        guest::read_words(memory, address, &mut header[..2])?;
        if header[1] != 16 {
            return Ok(INVALID_ARGUMENT);
        }
        guest::read_words(memory, address, &mut header)?;
        if header[2..5] != [2, 20, 11] {
            return Err(DispatchError::Unsupported);
        }
        let mut objects = [0; 44];
        guest::read_words(memory, header[5], &mut objects)?;
        // any-instance buttons bind in descriptor order.
        for (index, object) in objects.chunks_exact(4).enumerate() {
            let axis = index < 3;
            if object[0] == 0 {
                if axis {
                    return Err(DispatchError::Unsupported);
                }
            } else {
                guest::check(memory, object[0], 16, Access::Read)?;
                let mut guid = [0; 16];
                memory.read(u64::from(object[0]), &mut guid)?;
                let first = if axis {
                    0xe0 + u8::try_from(index).expect("bounded axis")
                } else {
                    0xf0
                };
                if guid
                    != [
                        first, 0x02, 0x6d, 0xa3, 0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45,
                        0x53, 0x54, 0, 0,
                    ]
                {
                    return Err(DispatchError::Unsupported);
                }
            }
            let offset =
                u32::try_from(if axis { index * 4 } else { index + 9 }).expect("bounded offset");
            let kind = if axis { 0x00ff_ff03 } else { 0x00ff_ff0c }
                | if index == 2 || index >= 5 {
                    0x8000_0000
                } else {
                    0
                };
            if object[1..] != [offset, kind, 0] {
                return Err(DispatchError::Unsupported);
            }
        }
        self.format = Some(Format::StandardMouse2);
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    fn words(memory: &mut GuestMemory, address: u32, values: &[u32]) {
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        memory.write(u64::from(address), &bytes).unwrap();
    }

    fn standard(memory: &mut GuestMemory) {
        words(memory, 0x1001, &[24, 16, 2, 20, 11, 0x1101]);
        for i in 0..11 {
            let (pointer, offset, kind) = if i < 3 {
                let mut guid = [
                    0xe0, 0x02, 0x6d, 0xa3, 0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53,
                    0x54, 0, 0,
                ];
                guid[0] += u8::try_from(i).unwrap();
                memory.write(u64::from(0x1201 + i * 16), &guid).unwrap();
                (
                    0x1201 + i * 16,
                    i * 4,
                    if i == 2 { 0x80ff_ff03 } else { 0x00ff_ff03 },
                )
            } else {
                (0, i + 9, if i >= 5 { 0x80ff_ff0c } else { 0x00ff_ff0c })
            };
            words(memory, 0x1101 + i * 16, &[pointer, offset, kind, 0]);
        }
    }

    #[test]
    fn failed_replacements_preserve_owned_format_and_recover_on_same_device() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        for configured in [false, true] {
            let mut device = Device::new();
            standard(&mut memory);
            if configured {
                assert_eq!(device.set_format(0x1001, &memory).ok(), Some(0));
            }
            let before = device.format;
            for (address, value) in [
                (0x1001, 0),
                (0x1005, 0),
                (0x1009, 1),
                (0x1015, 0x1fff),
                (0x1101, 0),
                (0x1101 + 160, 0x1ff8),
                (0x1101 + 164, 18),
                (0x1101 + 172, 1),
            ] {
                standard(&mut memory);
                words(&mut memory, address, &[value]);
                let result = device.set_format(0x1001, &memory);
                assert!(!matches!(result, Ok(0)));
                assert_eq!(device.format, before);
                assert_eq!(device.references, 1);
            }
            standard(&mut memory);
            assert_eq!(device.set_format(0x1001, &memory).ok(), Some(0));
            assert_eq!(device.format, Some(Format::StandardMouse2));
            memory.write(0x1000, &[0; 4096]).unwrap();
            assert_eq!(
                device.set_format(0x1001, &memory).ok(),
                Some(INVALID_ARGUMENT)
            );
            assert_eq!(device.format, Some(Format::StandardMouse2));
        }
        let mut first = Device::new();
        let second = Device::new();
        standard(&mut memory);
        assert_eq!(first.set_format(0x1001, &memory).ok(), Some(0));
        memory.unmap(0x1000, 4096).unwrap();
        assert!(first.set_format(0x1001, &memory).is_err());
        assert_eq!(first.format, Some(Format::StandardMouse2));
        assert_eq!(second.format, None);
        assert_eq!((first.references, second.references), (1, 1));
    }
}
