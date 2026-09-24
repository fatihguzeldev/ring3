use super::{Access, DispatchError, GuestMemory, INVALID_ARGUMENT, NULL_POINTER, guest};

const KEY: [u8; 16] = [
    0x20, 0x82, 0x72, 0x55, 0x3c, 0xd3, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    StandardKeyboard,
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
        if header[2..5] != [2, 256, 256] {
            return Err(DispatchError::Unsupported);
        }
        let mut objects = [0; 1024];
        guest::read_words(memory, header[5], &mut objects)?;
        let mut keys = [false; 256];
        for object in objects.chunks_exact(4) {
            if object[0] == 0 {
                return Err(DispatchError::Unsupported);
            }
            guest::check(memory, object[0], 16, Access::Read)?;
            let mut guid = [0; 16];
            memory.read(u64::from(object[0]), &mut guid)?;
            if guid != KEY
                || object[1] >= 256
                || object[2] != (0x8000_000c | object[1] << 8)
                || object[3] != 0
            {
                return Err(DispatchError::Unsupported);
            }
            let key = usize::try_from(object[1]).expect("bounded key offset");
            if keys[key] {
                return Err(DispatchError::Unsupported);
            }
            keys[key] = true;
        }
        self.format = Some(Format::StandardKeyboard);
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Call, Class, DEVICES, Input, KEYBOARD, OBJECTS};
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
        words(memory, 0x2000, &[24, 16, 2, 256, 256, 0x3000]);
        memory.write(0x4000, &KEY).unwrap();
        for key in 0..256 {
            words(
                memory,
                0x3000 + key * 16,
                &[0x4000, key, 0x8000_000c | key << 8, 0],
            );
        }
    }

    fn setup() -> (Input, GuestMemory) {
        let mut memory = GuestMemory::new(5);
        memory
            .map_zeroed(0x1000, 0x4000, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1020, &KEYBOARD).unwrap();
        let mut input = Input::default();
        assert_eq!(
            input
                .dispatch(Call::Create, &[1, 0x700, 0x1000, 0], &mut memory)
                .ok(),
            Some(0)
        );
        for _ in 0..2 {
            assert_eq!(
                input
                    .dispatch(
                        Call::CreateDevice,
                        &[OBJECTS, 0x1020, 0x1000, 0],
                        &mut memory
                    )
                    .ok(),
                Some(0)
            );
        }
        standard(&mut memory);
        (input, memory)
    }

    #[test]
    fn configuration_is_owned_per_device_and_failed_replacement_is_atomic() {
        let (mut input, mut memory) = setup();
        assert!(input.devices.iter().all(|device| device.format.is_none()));
        assert_eq!(
            input
                .dispatch(Call::Release(Class::Root), &[OBJECTS], &mut memory)
                .ok(),
            Some(0)
        );
        assert_eq!(
            input
                .dispatch(Call::SetDataFormat, &[DEVICES, 0x2000], &mut memory)
                .ok(),
            Some(0)
        );
        let configured = Some(Format::StandardKeyboard);
        for receiver in [DEVICES, DEVICES + 4] {
            assert_eq!(
                input
                    .dispatch(Call::SetDataFormat, &[receiver, 0], &mut memory)
                    .ok(),
                Some(NULL_POINTER)
            );
            words(&mut memory, 0x2000, &[23]);
            assert_eq!(
                input
                    .dispatch(Call::SetDataFormat, &[receiver, 0x2000], &mut memory)
                    .ok(),
                Some(INVALID_ARGUMENT)
            );
            standard(&mut memory);
            words(&mut memory, 0x3ffc, &[1]);
            assert!(matches!(
                input.dispatch(Call::SetDataFormat, &[receiver, 0x2000], &mut memory),
                Err(DispatchError::Unsupported)
            ));
            standard(&mut memory);
            words(&mut memory, 0x3ff0, &[0x5000]);
            assert!(matches!(
                input.dispatch(Call::SetDataFormat, &[receiver, 0x2000], &mut memory),
                Err(DispatchError::Memory(_))
            ));
            standard(&mut memory);
            assert_eq!(input.devices[0].format, configured);
            assert_eq!(input.devices[1].format, None);
        }
        assert_eq!(
            input
                .dispatch(Call::SetDataFormat, &[DEVICES + 4, 0x2000], &mut memory)
                .ok(),
            Some(0)
        );
        memory.write(0x1020, &super::super::INTERFACES[0]).unwrap();
        assert_eq!(
            input
                .dispatch(
                    Call::QueryInterface(Class::Keyboard),
                    &[DEVICES, 0x1020, 0x1000],
                    &mut memory
                )
                .ok(),
            Some(0)
        );
        words(&mut memory, 0x2000, &[0; 6]);
        assert!(
            input
                .devices
                .iter()
                .all(|device| device.format == configured)
        );
        memory.unmap(0x2000, 0x3000).unwrap();
        assert!(
            input
                .dispatch(Call::SetDataFormat, &[DEVICES, 0x2000], &mut memory)
                .is_err()
        );
        assert!(
            input
                .devices
                .iter()
                .all(|device| device.format == configured)
        );
        assert_eq!(input.devices[0].references, 2);
        assert_eq!(input.devices[1].references, 1);
        assert_eq!(input.roots, [0]);
        assert_eq!(memory.mapped_pages(), 2);
    }
}
