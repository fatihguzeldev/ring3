use super::{Access, Desktop, DispatchError, GuestMemory, INVALID_ARGUMENT, NULL_POINTER, guest};

const KEY: [u8; 16] = [
    0x20, 0x82, 0x72, 0x55, 0x3c, 0xd3, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    StandardKeyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CooperativeLevel {
    window: u32,
    suppress_windows_key: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BufferSize {
    requested: u32,
    capacity: u32,
}

pub(super) struct Device {
    pub(super) references: u32,
    format: Option<Format>,
    cooperative_level: Option<CooperativeLevel>,
    buffer_size: BufferSize,
}

impl Device {
    pub(super) fn new() -> Self {
        Self {
            references: 1,
            format: None,
            cooperative_level: None,
            buffer_size: BufferSize::default(),
        }
    }

    pub(super) fn set_property(
        &mut self,
        property: u32,
        address: u32,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        if property != 1 {
            return Err(DispatchError::Unsupported);
        }
        if address == 0 {
            return Ok(INVALID_ARGUMENT);
        }
        let mut header = [0; 5];
        guest::read_words(memory, address, &mut header[..2])?;
        if header[1] != 16 || header[0] != 20 {
            return Ok(INVALID_ARGUMENT);
        }
        guest::read_words(memory, address, &mut header)?;
        if header[3] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if header[2] != 0 {
            return Ok(INVALID_ARGUMENT);
        }
        self.buffer_size = BufferSize {
            requested: header[4],
            capacity: header[4].min(1024),
        };
        Ok(0)
    }

    pub(super) fn set_cooperative_level(
        &mut self,
        window: u32,
        flags: u32,
        desktop: &Desktop,
    ) -> Result<u32, DispatchError> {
        if matches!(flags & 0x3, 0 | 0x3) || matches!(flags & 0xc, 0 | 0xc) {
            return Ok(INVALID_ARGUMENT);
        }
        if !matches!(flags, 6 | 0x16) {
            return Err(DispatchError::Unsupported);
        }
        if desktop
            .window(window)
            .is_none_or(|window| window.style & 0x4000_0000 != 0)
        {
            return Ok(0x8007_0006);
        }
        self.cooperative_level = Some(CooperativeLevel {
            window,
            suppress_windows_key: flags & 0x10 != 0,
        });
        Ok(0)
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
    use super::super::super::desktop::Window;
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
                .dispatch(
                    Call::Create,
                    &[1, 0x700, 0x1000, 0],
                    &mut memory,
                    &Desktop::default()
                )
                .ok(),
            Some(0)
        );
        for _ in 0..2 {
            assert_eq!(
                input
                    .dispatch(
                        Call::CreateDevice,
                        &[OBJECTS, 0x1020, 0x1000, 0],
                        &mut memory,
                        &Desktop::default()
                    )
                    .ok(),
                Some(0)
            );
        }
        standard(&mut memory);
        (input, memory)
    }

    fn cooperate(
        input: &mut Input,
        memory: &mut GuestMemory,
        desktop: &Desktop,
        args: [u32; 3],
    ) -> Result<u32, DispatchError> {
        input.dispatch(Call::SetCooperativeLevel, &args, memory, desktop)
    }

    #[test]
    fn cooperative_settings_are_per_device_and_independent_of_format_and_root_refs() {
        let (mut input, mut memory) = setup();
        let mut desktop = Desktop::default();
        desktop.insert(0x7500_0004, Window::default());
        desktop.insert(
            0x7500_0008,
            Window {
                parent: 0x7500_0004,
                ..Window::default()
            },
        );
        assert!(
            input
                .devices
                .iter()
                .all(|device| device.cooperative_level.is_none())
        );
        assert_eq!(
            cooperate(&mut input, &mut memory, &desktop, [DEVICES, 0x7500_0004, 6]).ok(),
            Some(0)
        );
        assert_eq!(
            input
                .dispatch(
                    Call::Release(Class::Root),
                    &[OBJECTS],
                    &mut memory,
                    &desktop
                )
                .ok(),
            Some(0)
        );
        memory.write(0x1020, &super::super::INTERFACES[0]).unwrap();
        assert_eq!(
            input
                .dispatch(
                    Call::QueryInterface(Class::Keyboard),
                    &[DEVICES, 0x1020, 0x1000],
                    &mut memory,
                    &desktop
                )
                .ok(),
            Some(0)
        );
        assert_eq!(
            input
                .dispatch(
                    Call::SetDataFormat,
                    &[DEVICES, 0x2000],
                    &mut memory,
                    &desktop
                )
                .ok(),
            Some(0)
        );
        assert_eq!(
            cooperate(
                &mut input,
                &mut memory,
                &desktop,
                [DEVICES, 0x7500_0008, 0x16]
            )
            .ok(),
            Some(0)
        );
        assert_eq!(
            cooperate(
                &mut input,
                &mut memory,
                &desktop,
                [DEVICES + 4, 0x7500_0004, 6]
            )
            .ok(),
            Some(0)
        );
        assert_eq!(
            input.devices[0].cooperative_level,
            Some(CooperativeLevel {
                window: 0x7500_0008,
                suppress_windows_key: true
            })
        );
        assert_eq!(
            input.devices[1].cooperative_level,
            Some(CooperativeLevel {
                window: 0x7500_0004,
                suppress_windows_key: false
            })
        );
        assert_eq!(input.devices[0].format, Some(Format::StandardKeyboard));
        assert_eq!(input.devices[1].format, None);
        assert_eq!(input.devices[0].references, 2);
        assert_eq!(input.devices[1].references, 1);
        assert_eq!(input.roots, [0]);
    }

    #[test]
    fn cooperative_failures_preserve_settings_and_do_not_keep_windows_alive() {
        let (mut input, mut memory) = setup();
        let mut desktop = Desktop::default();
        desktop.insert(0x7500_0004, Window::default());
        desktop.insert_child(
            0x7500_0008,
            Window {
                parent: 0x7500_0004,
                style: 0x4000_0000,
                ..Window::default()
            },
        );
        assert_eq!(
            cooperate(
                &mut input,
                &mut memory,
                &desktop,
                [DEVICES, 0x7500_0004, 0x16]
            )
            .ok(),
            Some(0)
        );
        let initial = input.devices[0].cooperative_level;
        for receiver in [DEVICES, DEVICES + 4] {
            assert_eq!(
                cooperate(
                    &mut input,
                    &mut memory,
                    &desktop,
                    [receiver, 0x7500_0008, 0]
                )
                .ok(),
                Some(INVALID_ARGUMENT)
            );
            for flags in [5, 9, 10, 0x26] {
                assert!(matches!(
                    cooperate(
                        &mut input,
                        &mut memory,
                        &desktop,
                        [receiver, 0x7500_0008, flags]
                    ),
                    Err(DispatchError::Unsupported)
                ));
            }
            for window in [0, 1, 0x7500_0008, 0xdead_beef] {
                assert_eq!(
                    cooperate(&mut input, &mut memory, &desktop, [receiver, window, 6]).ok(),
                    Some(0x8007_0006)
                );
            }
            assert_eq!(input.devices[0].cooperative_level, initial);
            assert_eq!(input.devices[1].cooperative_level, None);
        }
        desktop.remove(0x7500_0004);
        assert_eq!(
            cooperate(&mut input, &mut memory, &desktop, [DEVICES, 0x7500_0004, 6]).ok(),
            Some(0x8007_0006)
        );
        assert_eq!(input.devices[0].cooperative_level, initial);
        assert!(desktop.window(0x7500_0004).is_none());
        assert_eq!(input.devices[0].references, 1);
        assert_eq!(input.devices[1].references, 1);
    }

    fn configured_first() -> (Input, GuestMemory) {
        let (mut input, mut memory) = setup();
        assert!(input.devices.iter().all(|device| device.format.is_none()));
        assert_eq!(
            input
                .dispatch(
                    Call::Release(Class::Root),
                    &[OBJECTS],
                    &mut memory,
                    &Desktop::default()
                )
                .ok(),
            Some(0)
        );
        assert_eq!(
            input
                .dispatch(
                    Call::SetDataFormat,
                    &[DEVICES, 0x2000],
                    &mut memory,
                    &Desktop::default()
                )
                .ok(),
            Some(0)
        );
        (input, memory)
    }

    fn property(
        input: &mut Input,
        memory: &mut GuestMemory,
        args: [u32; 3],
    ) -> Result<u32, DispatchError> {
        input.dispatch(Call::SetProperty, &args, memory, &Desktop::default())
    }

    #[test]
    fn buffer_capacity_preserves_requested_values_and_other_device_state() {
        let (mut input, mut memory) = configured_first();
        assert!(
            input
                .devices
                .iter()
                .all(|device| device.buffer_size == BufferSize::default())
        );
        let mut desktop = Desktop::default();
        desktop.insert(0x7500_0004, Window::default());
        assert_eq!(
            cooperate(&mut input, &mut memory, &desktop, [DEVICES, 0x7500_0004, 6]).ok(),
            Some(0)
        );
        let cooperative = input.devices[0].cooperative_level;
        memory.write(0x1020, &super::super::INTERFACES[0]).unwrap();
        assert_eq!(
            input
                .dispatch(
                    Call::QueryInterface(Class::Keyboard),
                    &[DEVICES, 0x1020, 0x1000],
                    &mut memory,
                    &desktop
                )
                .ok(),
            Some(0)
        );
        for (requested, capacity) in [
            (0, 0),
            (1, 1),
            (16, 16),
            (1024, 1024),
            (1025, 1024),
            (u32::MAX, 1024),
            (0, 0),
        ] {
            words(&mut memory, 0x1101, &[20, 16, 0, 0, requested]);
            assert_eq!(
                property(&mut input, &mut memory, [DEVICES, 1, 0x1101]).ok(),
                Some(0)
            );
            assert_eq!(
                input.devices[0].buffer_size,
                BufferSize {
                    requested,
                    capacity
                }
            );
            assert_eq!(input.devices[1].buffer_size, BufferSize::default());
        }
        assert_eq!(input.devices[0].format, Some(Format::StandardKeyboard));
        assert_eq!(input.devices[1].format, None);
        assert_eq!(input.devices[0].cooperative_level, cooperative);
        assert_eq!(input.devices[1].cooperative_level, None);
        assert_eq!(input.devices[0].references, 2);
        assert_eq!(input.devices[1].references, 1);
        assert_eq!(input.roots, [0]);
    }

    #[test]
    fn failed_buffer_replacement_preserves_values_and_recovers_without_borrowing() {
        let (mut input, mut memory) = configured_first();
        words(&mut memory, 0x1101, &[20, 16, 0, 0, 16]);
        assert_eq!(
            property(&mut input, &mut memory, [DEVICES, 1, 0x1101]).ok(),
            Some(0)
        );
        let first = input.devices[0].buffer_size;
        let pages = memory.mapped_pages();
        for receiver in [DEVICES, DEVICES + 4] {
            assert!(matches!(
                property(&mut input, &mut memory, [receiver, 2, 0x5000]),
                Err(DispatchError::Unsupported)
            ));
            assert_eq!(
                property(&mut input, &mut memory, [receiver, 1, 0]).ok(),
                Some(INVALID_ARGUMENT)
            );
            for header in [[19, 16, 0, 0, 99], [20, 15, 0, 0, 99], [20, 16, 1, 0, 99]] {
                words(&mut memory, 0x1101, &header);
                assert_eq!(
                    property(&mut input, &mut memory, [receiver, 1, 0x1101]).ok(),
                    Some(INVALID_ARGUMENT)
                );
            }
            words(&mut memory, 0x1101, &[20, 16, 0, 1, 99]);
            assert!(matches!(
                property(&mut input, &mut memory, [receiver, 1, 0x1101]),
                Err(DispatchError::Unsupported)
            ));
            words(&mut memory, 0x4ff8, &[20, 16]);
            assert!(matches!(
                property(&mut input, &mut memory, [receiver, 1, 0x4ff8]),
                Err(DispatchError::Memory(_))
            ));
            assert_eq!(input.devices[0].buffer_size, first);
            assert_eq!(input.devices[1].buffer_size, BufferSize::default());
        }
        words(&mut memory, 0x1101, &[20, 16, 0, 0, u32::MAX]);
        assert_eq!(
            property(&mut input, &mut memory, [DEVICES + 4, 1, 0x1101]).ok(),
            Some(0)
        );
        words(&mut memory, 0x1101, &[0; 5]);
        memory.unmap(0x1000, 4096).unwrap();
        assert_eq!(input.devices[0].buffer_size, first);
        assert_eq!(
            input.devices[1].buffer_size,
            BufferSize {
                requested: u32::MAX,
                capacity: 1024
            }
        );
        assert_eq!(memory.mapped_pages(), pages - 1);
        assert_eq!(input.devices[0].references, 1);
        assert_eq!(input.devices[1].references, 1);
        assert_eq!(input.roots, [0]);
    }

    #[test]
    fn failed_format_replacement_preserves_configured_and_unconfigured_devices() {
        let (mut input, mut memory) = configured_first();
        let configured = Some(Format::StandardKeyboard);
        for receiver in [DEVICES, DEVICES + 4] {
            assert_eq!(
                input
                    .dispatch(
                        Call::SetDataFormat,
                        &[receiver, 0],
                        &mut memory,
                        &Desktop::default()
                    )
                    .ok(),
                Some(NULL_POINTER)
            );
            words(&mut memory, 0x2000, &[23]);
            assert_eq!(
                input
                    .dispatch(
                        Call::SetDataFormat,
                        &[receiver, 0x2000],
                        &mut memory,
                        &Desktop::default()
                    )
                    .ok(),
                Some(INVALID_ARGUMENT)
            );
            standard(&mut memory);
            words(&mut memory, 0x3ffc, &[1]);
            assert!(matches!(
                input.dispatch(
                    Call::SetDataFormat,
                    &[receiver, 0x2000],
                    &mut memory,
                    &Desktop::default()
                ),
                Err(DispatchError::Unsupported)
            ));
            standard(&mut memory);
            words(&mut memory, 0x3ff0, &[0x5000]);
            assert!(matches!(
                input.dispatch(
                    Call::SetDataFormat,
                    &[receiver, 0x2000],
                    &mut memory,
                    &Desktop::default()
                ),
                Err(DispatchError::Memory(_))
            ));
            standard(&mut memory);
            assert_eq!(input.devices[0].format, configured);
            assert_eq!(input.devices[1].format, None);
        }
        assert_eq!(
            input
                .dispatch(
                    Call::SetDataFormat,
                    &[DEVICES + 4, 0x2000],
                    &mut memory,
                    &Desktop::default()
                )
                .ok(),
            Some(0)
        );
        assert_eq!(input.devices[1].format, configured);
        assert_eq!(input.devices[0].references, 1);
        assert_eq!(input.devices[1].references, 1);
        assert_eq!(input.roots, [0]);
    }

    #[test]
    fn format_configuration_outlives_guest_storage_and_creator() {
        let (mut input, mut memory) = configured_first();
        let configured = Some(Format::StandardKeyboard);
        assert_eq!(
            input
                .dispatch(
                    Call::SetDataFormat,
                    &[DEVICES + 4, 0x2000],
                    &mut memory,
                    &Desktop::default()
                )
                .ok(),
            Some(0)
        );
        memory.write(0x1020, &super::super::INTERFACES[0]).unwrap();
        assert_eq!(
            input
                .dispatch(
                    Call::QueryInterface(Class::Keyboard),
                    &[DEVICES, 0x1020, 0x1000],
                    &mut memory,
                    &Desktop::default()
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
                .dispatch(
                    Call::SetDataFormat,
                    &[DEVICES, 0x2000],
                    &mut memory,
                    &Desktop::default()
                )
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
