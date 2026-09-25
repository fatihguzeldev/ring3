use super::buffer::BufferSize;
use super::{Access, Desktop, DispatchError, GuestMemory, INVALID_ARGUMENT, NULL_POINTER, guest};

const KEY: [u8; 16] = [
    0x20, 0x82, 0x72, 0x55, 0x3c, 0xd3, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];
const ACQUIRED: u32 = 0x8007_00aa;

pub(super) struct State(pub(super) [u8; 256]);

impl Default for State {
    fn default() -> Self {
        Self([0; 256])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    StandardKeyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CooperativeLevel {
    window: u32,
    suppress_windows_key: bool,
}

pub(super) struct Device {
    pub(super) references: u32,
    format: Option<Format>,
    cooperative_level: Option<CooperativeLevel>,
    buffer_size: BufferSize,
    acquired_epoch: Option<u64>,
}

impl Device {
    pub(super) fn new() -> Self {
        Self {
            references: 1,
            format: None,
            cooperative_level: None,
            buffer_size: BufferSize::default(),
            acquired_epoch: None,
        }
    }

    fn is_acquired(&self, desktop: &Desktop) -> bool {
        let (Some(epoch), Some(level)) = (self.acquired_epoch, self.cooperative_level) else {
            return false;
        };
        self.references != 0
            && desktop.activation() == (level.window, Some(epoch))
            && desktop
                .window(level.window)
                .is_some_and(|window| window.style & 0x4000_0000 == 0)
    }

    pub(super) fn acquire(&mut self, desktop: &Desktop) -> Result<u32, DispatchError> {
        if self.is_acquired(desktop) {
            return Ok(1);
        }
        if self.format.is_none() {
            return Ok(INVALID_ARGUMENT);
        }
        let level = self.cooperative_level.ok_or(DispatchError::Unsupported)?;
        if desktop
            .window(level.window)
            .is_none_or(|window| window.style & 0x4000_0000 != 0)
        {
            return Ok(INVALID_ARGUMENT);
        }
        let (active, epoch) = desktop.activation();
        if active != level.window {
            return Ok(0x8007_0005);
        }
        self.acquired_epoch = Some(epoch.ok_or(DispatchError::Unsupported)?);
        Ok(0)
    }

    pub(super) fn unacquire(&mut self, desktop: &Desktop) -> u32 {
        let previous = self.is_acquired(desktop);
        self.acquired_epoch = None;
        u32::from(!previous)
    }

    pub(super) fn get_state(
        &self,
        size: u32,
        address: u32,
        state: &State,
        memory: &mut GuestMemory,
        desktop: &Desktop,
    ) -> Result<u32, DispatchError> {
        if size != 256 || address == 0 {
            return Ok(INVALID_ARGUMENT);
        }
        if self.acquired_epoch.is_none() {
            return Ok(0x8007_000c);
        }
        if !self.is_acquired(desktop) {
            return Ok(0x8007_001e);
        }
        guest::check(memory, address, 256, Access::Write)?;
        memory.write(u64::from(address), &state.0)?;
        Ok(0)
    }

    pub(super) fn set_property(
        &mut self,
        property: u32,
        address: u32,
        memory: &GuestMemory,
        desktop: &Desktop,
    ) -> Result<u32, DispatchError> {
        let acquired = self.is_acquired(desktop);
        self.buffer_size.set(property, address, memory, acquired)
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
        if self.is_acquired(desktop) {
            return Ok(ACQUIRED);
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
        desktop: &Desktop,
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
        if self.is_acquired(desktop) {
            return Ok(ACQUIRED);
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

    fn foreground() -> Desktop {
        let mut desktop = Desktop::default();
        for handle in [4, 8] {
            desktop.insert(
                handle,
                Window {
                    style: 0x1000_0000,
                    ..Window::default()
                },
            );
        }
        desktop.activate_created(4);
        desktop
    }

    #[test]
    fn nonexclusive_acquisition_and_alias_lifetimes_are_independent() {
        let (mut input, mut memory) = configured_first();
        let desktop = foreground();
        assert!(
            input
                .devices
                .iter()
                .all(|device| !device.is_acquired(&desktop))
        );
        assert_eq!(
            input.devices[1].set_format(0x2000, &memory, &desktop).ok(),
            Some(0)
        );
        for device in &mut input.devices {
            assert_eq!(device.set_cooperative_level(4, 6, &desktop).ok(), Some(0));
            assert_eq!(device.acquire(&desktop).ok(), Some(0));
        }
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
        assert_eq!(input.devices[0].references, 2);
        assert!(
            input
                .devices
                .iter()
                .all(|device| device.is_acquired(&desktop))
        );
        assert_eq!(input.devices[0].unacquire(&desktop), 0);
        assert!(!input.devices[0].is_acquired(&desktop));
        assert!(input.devices[1].is_acquired(&desktop));
        assert_eq!(input.devices[0].acquire(&desktop).ok(), Some(0));
        for (receiver, expected) in [(DEVICES, 1), (DEVICES, 0), (DEVICES + 4, 0)] {
            assert_eq!(
                input
                    .dispatch(
                        Call::Release(Class::Keyboard),
                        &[receiver],
                        &mut memory,
                        &desktop
                    )
                    .ok(),
                Some(expected)
            );
        }
        assert!(
            input
                .devices
                .iter()
                .all(|device| !device.is_acquired(&desktop))
        );
        assert_eq!(input.roots, [0]);
    }

    fn standard_mouse(memory: &mut GuestMemory) {
        words(memory, 0x2100, &[24, 16, 2, 20, 11, 0x2200]);
        for index in 0..11 {
            let (guid, offset, kind) = if index < 3 {
                let mut guid = [
                    0xe0, 0x02, 0x6d, 0xa3, 0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53,
                    0x54, 0, 0,
                ];
                guid[0] += u8::try_from(index).unwrap();
                memory.write(u64::from(0x2300 + index * 16), &guid).unwrap();
                (
                    0x2300 + index * 16,
                    index * 4,
                    if index == 2 { 0x80ff_ff03 } else { 0x00ff_ff03 },
                )
            } else {
                (
                    0,
                    index + 9,
                    if index >= 5 { 0x80ff_ff0c } else { 0x00ff_ff0c },
                )
            };
            words(memory, 0x2200 + index * 16, &[guid, offset, kind, 0]);
        }
    }

    #[test]
    fn mouse_configuration_and_lifetime_do_not_change_acquired_keyboard() {
        let (mut input, mut memory) = setup();
        let desktop = foreground();
        let device = &mut input.devices[0];
        assert_eq!(device.set_format(0x2000, &memory, &desktop).ok(), Some(0));
        assert_eq!(
            device.set_cooperative_level(4, 0x16, &desktop).ok(),
            Some(0)
        );
        words(&mut memory, 0x2100, &[20, 16, 0, 0, 16]);
        assert_eq!(
            device.set_property(1, 0x2100, &memory, &desktop).ok(),
            Some(0)
        );
        assert_eq!(device.acquire(&desktop).ok(), Some(0));
        let before = (
            device.references,
            device.format,
            device.cooperative_level,
            device.buffer_size,
            device.acquired_epoch,
        );
        let activation = desktop.activation();
        let pages = memory.mapped_pages();
        memory.write(0x1020, &super::super::MOUSE).unwrap();
        assert_eq!(
            input
                .dispatch(
                    Call::CreateDevice,
                    &[OBJECTS, 0x1020, 0x1000, 0],
                    &mut memory,
                    &desktop
                )
                .ok(),
            Some(0)
        );
        standard_mouse(&mut memory);
        memory
            .write(0x1020, &super::super::DEVICE_INTERFACES[0])
            .unwrap();
        let mouse = super::super::MICE;
        words(&mut memory, 0x2400, &[44]);
        words(&mut memory, 0x2500, &[20, 16, 0, 0, 32]);
        words(&mut memory, 0x2600, &[20, 16, 8, 1, 0]);
        for (call, args, expected) in [
            (Call::SetMouseDataFormat, vec![mouse, 0x2100], 0),
            (Call::SetMouseCooperativeLevel, vec![mouse, 4, 5], 0),
            (Call::MouseCapabilities, vec![mouse, 0x2400], 0),
            (Call::SetMouseProperty, vec![mouse, 1, 0x2500], 0),
            (Call::GetMouseProperty, vec![mouse, 3, 0x2600], 0),
            (Call::AcquireMouse, vec![mouse], 0),
            (Call::AcquireMouse, vec![mouse], 1),
            (Call::UnacquireMouse, vec![mouse], 0),
            (Call::AcquireMouse, vec![mouse], 0),
            (
                Call::QueryInterface(Class::Mouse),
                vec![mouse, 0x1020, 0x1000],
                0,
            ),
            (Call::AddRef(Class::Mouse), vec![mouse], 3),
            (Call::Release(Class::Root), vec![OBJECTS], 0),
            (Call::Release(Class::Mouse), vec![mouse], 2),
            (Call::Release(Class::Mouse), vec![mouse], 1),
            (Call::Release(Class::Mouse), vec![mouse], 0),
        ] {
            assert_eq!(
                input.dispatch(call, &args, &mut memory, &desktop).ok(),
                Some(expected)
            );
        }
        let device = &mut input.devices[0];
        assert_eq!(
            before,
            (
                device.references,
                device.format,
                device.cooperative_level,
                device.buffer_size,
                device.acquired_epoch
            )
        );
        assert!(device.is_acquired(&desktop));
        assert_eq!(device.acquire(&desktop).ok(), Some(1));
        assert_eq!(desktop.activation(), activation);
        assert_eq!(memory.mapped_pages(), pages);
        assert_eq!(input.devices[1].references, 1);
        assert!(!input.devices[1].is_acquired(&desktop));
        assert_eq!(input.mice[0].references, 0);
    }

    #[test]
    fn acquired_settings_remain_owned_until_explicit_release_or_focus_loss() {
        let (mut input, mut memory) = configured_first();
        let mut desktop = foreground();
        let device = &mut input.devices[0];
        assert_eq!(device.set_cooperative_level(4, 6, &desktop).ok(), Some(0));
        words(&mut memory, 0x1101, &[20, 16, 0, 0, 16]);
        assert_eq!(
            device.set_property(1, 0x1101, &memory, &desktop).ok(),
            Some(0)
        );
        assert_eq!(device.acquire(&desktop).ok(), Some(0));
        let before = (
            device.format,
            device.cooperative_level,
            device.buffer_size,
            device.acquired_epoch,
        );
        assert_eq!(
            device.set_format(0x2000, &memory, &desktop).ok(),
            Some(ACQUIRED)
        );
        assert_eq!(
            device.set_cooperative_level(8, 0x16, &desktop).ok(),
            Some(ACQUIRED)
        );
        words(&mut memory, 0x1101, &[20, 16, 0, 0, u32::MAX]);
        assert_eq!(
            device.set_property(1, 0x1101, &memory, &desktop).ok(),
            Some(ACQUIRED)
        );
        assert!(matches!(
            device.set_property(1, 0x5000, &memory, &desktop),
            Err(DispatchError::Memory(_))
        ));
        assert_eq!(
            (
                device.format,
                device.cooperative_level,
                device.buffer_size,
                device.acquired_epoch
            ),
            before
        );
        assert!(desktop.activate_foreground(8));
        assert!(desktop.activate_foreground(4));
        assert!(!device.is_acquired(&desktop));
        assert_eq!(device.set_format(0x2000, &memory, &desktop).ok(), Some(0));
        assert_eq!(
            device.set_property(1, 0x1101, &memory, &desktop).ok(),
            Some(0)
        );
        assert_eq!(
            device.set_cooperative_level(4, 0x16, &desktop).ok(),
            Some(0)
        );
        assert_eq!(device.acquire(&desktop).ok(), Some(0));
        assert!(device.is_acquired(&desktop));
        assert_ne!(device.acquired_epoch, before.3);
        assert_eq!(
            device.buffer_size,
            BufferSize {
                requested: u32::MAX,
                capacity: 1024
            }
        );
        assert_eq!(device.unacquire(&desktop), 0);
        assert_eq!(device.unacquire(&desktop), 1);
        assert_eq!(device.acquired_epoch, None);
        assert_eq!(input.devices[0].references, 1);
        assert_eq!(input.devices[1].buffer_size, BufferSize::default());
        assert!(!input.devices[1].is_acquired(&desktop));
        assert_eq!(input.roots, [0]);
    }

    #[test]
    fn acquisition_revalidates_child_and_removed_windows_without_losing_configuration() {
        let (mut input, _) = configured_first();
        let mut desktop = foreground();
        let device = &mut input.devices[0];
        assert_eq!(device.set_cooperative_level(4, 6, &desktop).ok(), Some(0));
        let configured = device.cooperative_level;
        desktop.window_mut(4).unwrap().style |= 0x4000_0000;
        assert_eq!(device.acquire(&desktop).ok(), Some(INVALID_ARGUMENT));
        assert_eq!(device.acquired_epoch, None);
        desktop.window_mut(4).unwrap().style &= !0x4000_0000;
        assert_eq!(device.acquire(&desktop).ok(), Some(0));
        desktop.remove(4);
        assert!(!device.is_acquired(&desktop));
        assert_eq!(device.acquire(&desktop).ok(), Some(INVALID_ARGUMENT));
        assert_eq!(device.unacquire(&desktop), 1);
        assert_eq!(device.cooperative_level, configured);
        assert_eq!(device.format, Some(Format::StandardKeyboard));
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
