use super::super::MemoryError;
use super::buffer::BufferSize;
use super::{Access, Desktop, DispatchError, GuestMemory, INVALID_ARGUMENT, NULL_POINTER, guest};

pub(super) fn capabilities(address: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    if address == 0 {
        return Ok(NULL_POINTER);
    }
    let mut size = [0];
    guest::read_words(memory, address, &mut size)?;
    let length = match size[0] {
        24 => 24,
        44 => 44,
        _ => return Ok(INVALID_ARGUMENT),
    };
    guest::check(memory, address, length, Access::Write)?;
    // this process-owned virtual device exposes eight buttons, not host hardware counts.
    let words = [size[0], 5, 0x202, 3, 8, 0, 0, 0, 0, 0, 0];
    let mut bytes = [0; 44];
    for (word, output) in words.into_iter().zip(bytes.chunks_exact_mut(4)) {
        output.copy_from_slice(&word.to_le_bytes());
    }
    memory.write(u64::from(address), &bytes[..length])?;
    Ok(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    StandardMouse2,
}

pub(super) struct Device {
    pub(super) references: u32,
    format: Option<Format>,
    cooperative_window: Option<u32>,
    buffer_size: BufferSize,
    acquired_epoch: Option<u64>,
}

impl Device {
    pub(super) fn new() -> Self {
        Self {
            references: 1,
            format: None,
            cooperative_window: None,
            buffer_size: BufferSize::default(),
            acquired_epoch: None,
        }
    }

    pub(super) fn is_acquired(&self, desktop: &Desktop) -> bool {
        let (Some(epoch), Some(window)) = (self.acquired_epoch, self.cooperative_window) else {
            return false;
        };
        self.references != 0
            && desktop.activation() == (window, Some(epoch))
            && desktop
                .window(window)
                .is_some_and(|window| window.style & 0x4000_0000 == 0)
    }

    pub(super) fn acquire(
        &mut self,
        desktop: &Desktop,
        occupied: impl FnOnce() -> bool,
    ) -> Result<u32, DispatchError> {
        if self.is_acquired(desktop) {
            return Ok(1);
        }
        if self.format.is_none() {
            return Ok(INVALID_ARGUMENT);
        }
        let window = self.cooperative_window.ok_or(DispatchError::Unsupported)?;
        if desktop
            .window(window)
            .is_none_or(|window| window.style & 0x4000_0000 != 0)
        {
            return Ok(INVALID_ARGUMENT);
        }
        let (active, epoch) = desktop.activation();
        if active != window {
            return Ok(0x8007_0005);
        }
        let epoch = epoch.ok_or(DispatchError::Unsupported)?;
        if occupied() {
            return Ok(0x8007_0005);
        }
        self.acquired_epoch = Some(epoch);
        Ok(0)
    }

    pub(super) fn unacquire(&mut self, desktop: &Desktop) -> u32 {
        let previous = self.is_acquired(desktop);
        self.acquired_epoch = None;
        u32::from(!previous)
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

    pub(super) fn get_property(
        &self,
        property: u32,
        address: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if property != 3 {
            return Err(DispatchError::Unsupported);
        }
        if address == 0 {
            return Ok(INVALID_ARGUMENT);
        }
        let mut header = [0; 4];
        guest::read_words(memory, address, &mut header[..2])?;
        if header[1] != 16 || header[0] != 20 {
            return Ok(INVALID_ARGUMENT);
        }
        guest::read_words(memory, address, &mut header)?;
        if header[3] > 3 {
            return Ok(INVALID_ARGUMENT);
        }
        if header[3] != 1 {
            return Err(DispatchError::Unsupported);
        }
        if self.format.is_none() {
            return Ok(0x8007_0002);
        }
        // virtual wheel steps use wheel-delta units; x/y use single units.
        let granularity = match header[2] {
            0 | 4 => 1,
            8 => 120,
            12..=19 => return Err(DispatchError::Unsupported),
            _ => return Ok(0x8007_0002),
        };
        let output = address
            .checked_add(16)
            .ok_or(MemoryError::AddressOverflow)?;
        guest::write_word(memory, output, granularity)?;
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
        if flags != 5 {
            return Err(DispatchError::Unsupported);
        }
        if desktop
            .window(window)
            .is_none_or(|window| window.style & 0x4000_0000 != 0)
        {
            return Ok(0x8007_0006);
        }
        if self.is_acquired(desktop) {
            return Ok(0x8007_00aa);
        }
        self.cooperative_window = Some(window);
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
            return Ok(0x8007_00aa);
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
    use super::super::super::desktop::Window;
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

    fn desktop() -> Desktop {
        let mut desktop = Desktop::default();
        desktop.insert(4, Window::default());
        desktop.insert(
            8,
            Window {
                parent: 4,
                ..Window::default()
            },
        );
        desktop.insert(
            12,
            Window {
                parent: 4,
                style: 0x4000_0000,
                ..Window::default()
            },
        );
        desktop.activate_created(4);
        desktop
    }

    #[test]
    fn acquisition_keeps_owned_settings_and_validates_before_competing_claims() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let mut desktop = desktop();
        assert_eq!(desktop.show_activated(4), Some(0));
        let mut device = Device::new();
        let not_reached = || panic!("conflict check before device validation");
        assert_eq!(
            device.acquire(&desktop, not_reached).ok(),
            Some(INVALID_ARGUMENT)
        );
        standard(&mut memory);
        assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        assert!(matches!(
            device.acquire(&desktop, not_reached),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(device.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        words(&mut memory, 0x1301, &[20, 16, 0, 0, 32]);
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0)
        );
        let before = (
            device.references,
            device.format,
            device.cooperative_window,
            device.buffer_size,
        );
        assert_eq!(device.acquire(&desktop, || true).ok(), Some(0x8007_0005));
        assert_eq!(device.acquired_epoch, None);
        assert_eq!(device.acquire(&desktop, || false).ok(), Some(0));
        assert_eq!(device.acquire(&desktop, not_reached).ok(), Some(1));
        assert_eq!(
            device.set_format(0x1001, &memory, &desktop).ok(),
            Some(0x8007_00aa)
        );
        assert_eq!(
            device.set_cooperative_level(8, 5, &desktop).ok(),
            Some(0x8007_00aa)
        );
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0x8007_00aa)
        );
        assert_eq!(
            (
                device.references,
                device.format,
                device.cooperative_window,
                device.buffer_size
            ),
            before
        );
        memory.unmap(0x1000, 4096).unwrap();
        assert!(device.is_acquired(&desktop));
        assert_eq!(device.unacquire(&desktop), 0);
        assert_eq!(device.unacquire(&desktop), 1);
        assert_eq!(device.acquire(&desktop, || false).ok(), Some(0));
        assert_eq!(
            (
                device.references,
                device.format,
                device.cooperative_window,
                device.buffer_size
            ),
            before
        );
        device.references = 0;
        assert!(!device.is_acquired(&desktop));
        assert_eq!(memory.mapped_pages(), 0);
    }

    #[test]
    fn focus_child_conversion_and_window_removal_invalidate_mouse_access() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        standard(&mut memory);
        let mut desktop = desktop();
        assert_eq!(desktop.show_activated(4), Some(0));
        let mut device = Device::new();
        assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        assert_eq!(device.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        assert_eq!(device.acquire(&desktop, || false).ok(), Some(0));
        let old = device.acquired_epoch;
        desktop.show_activated(8);
        assert!(!device.is_acquired(&desktop));
        assert_eq!(
            device.acquire(&desktop, || panic!("inactive window")).ok(),
            Some(0x8007_0005)
        );
        desktop.show_activated(4);
        assert!(!device.is_acquired(&desktop));
        assert_eq!(device.acquire(&desktop, || false).ok(), Some(0));
        assert_ne!(device.acquired_epoch, old);
        desktop.window_mut(4).unwrap().style |= 0x4000_0000;
        assert!(!device.is_acquired(&desktop));
        assert_eq!(
            device.acquire(&desktop, || panic!("child window")).ok(),
            Some(INVALID_ARGUMENT)
        );
        assert_eq!(device.unacquire(&desktop), 1);
        desktop.remove(4);
        assert_eq!(
            device.acquire(&desktop, || panic!("removed window")).ok(),
            Some(INVALID_ARGUMENT)
        );
        assert_eq!(device.cooperative_window, Some(4));
        assert_eq!(device.set_cooperative_level(8, 5, &desktop).ok(), Some(0));
        desktop.show_activated(8);
        assert_eq!(device.acquire(&desktop, || false).ok(), Some(0));
        assert!(device.is_acquired(&desktop));
        assert_eq!(device.format, Some(Format::StandardMouse2));
    }

    #[test]
    fn granularity_queries_preserve_owned_mouse_configuration_and_other_devices() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let mut device = Device::new();
        let other = Device::new();
        let desktop = desktop();
        standard(&mut memory);
        assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        assert_eq!(device.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        words(&mut memory, 0x1301, &[20, 16, 0, 0, 32]);
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0)
        );
        let before = (
            device.references,
            device.format,
            device.cooperative_window,
            device.buffer_size,
        );
        let activation = desktop.activation();
        memory.write(0x1000, &[0; 4096]).unwrap();
        for (offset, expected) in [
            (0, Some(0)),
            (8, Some(0)),
            (12, None),
            (20, Some(0x8007_0002)),
        ] {
            words(&mut memory, 0x1301, &[20, 16, offset, 1, 77]);
            assert_eq!(device.get_property(3, 0x1301, &mut memory).ok(), expected);
            assert_eq!(
                (
                    device.references,
                    device.format,
                    device.cooperative_window,
                    device.buffer_size
                ),
                before
            );
        }
        assert!(device.get_property(3, 0x5000, &mut memory).is_err());
        assert_eq!(
            (
                device.references,
                device.format,
                device.cooperative_window,
                device.buffer_size
            ),
            before
        );
        assert_eq!(
            (
                other.references,
                other.format,
                other.cooperative_window,
                other.buffer_size
            ),
            (1, None, None, BufferSize::default())
        );
        assert_eq!(desktop.activation(), activation);
        memory.unmap(0x1000, 4096).unwrap();
        memory
            .map_zeroed(0x3000, 4096, Permissions::READ_WRITE)
            .unwrap();
        words(&mut memory, 0x3001, &[20, 16, 8, 1, 77]);
        assert_eq!(device.get_property(3, 0x3001, &mut memory).ok(), Some(0));
        let mut value = [0];
        guest::read_words(&memory, 0x3011, &mut value).unwrap();
        assert_eq!(value, [120]);
        assert_eq!(memory.mapped_pages(), 1);
    }

    #[test]
    fn buffer_capacity_is_owned_per_mouse_and_keeps_other_configuration() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let mut device = Device::new();
        let second = Device::new();
        assert_eq!(device.buffer_size, BufferSize::default());
        let desktop = desktop();
        standard(&mut memory);
        assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        assert_eq!(device.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        let activation = desktop.activation();
        for requested in [0, 1, 16, 1024, 1025, u32::MAX, 0] {
            words(&mut memory, 0x1301, &[20, 16, 0, 0, requested]);
            assert_eq!(
                device.set_property(1, 0x1301, &memory, &desktop).ok(),
                Some(0)
            );
            assert_eq!(
                device.buffer_size,
                BufferSize {
                    requested,
                    capacity: requested.min(1024)
                }
            );
            memory.write(0x1301, &[0; 20]).unwrap();
            assert_eq!(device.buffer_size.requested, requested);
            assert_eq!(device.buffer_size.capacity, requested.min(1024));
            assert_eq!(second.buffer_size, BufferSize::default());
            assert_eq!(device.format, Some(Format::StandardMouse2));
            assert_eq!(device.cooperative_window, Some(4));
            assert_eq!((device.references, second.references), (1, 1));
        }
        words(&mut memory, 0x1301, &[20, 16, 0, 0, 16]);
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0)
        );
        memory.unmap(0x1000, 4096).unwrap();
        assert_eq!(
            device.buffer_size,
            BufferSize {
                requested: 16,
                capacity: 16
            }
        );
        assert_eq!(desktop.activation(), activation);
    }

    #[test]
    fn failed_buffer_replacement_keeps_same_mouse_setting_and_can_recover() {
        let desktop = desktop();
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let mut device = Device::new();
        words(&mut memory, 0x1301, &[20, 16, 0, 0, 16]);
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0)
        );
        let before = device.buffer_size;
        for header in [
            [19, 16, 0, 0, 32],
            [20, 15, 0, 0, 32],
            [20, 16, 1, 0, 32],
            [20, 16, 0, 1, 32],
        ] {
            words(&mut memory, 0x1301, &header);
            assert!(!matches!(
                device.set_property(1, 0x1301, &memory, &desktop),
                Ok(0)
            ));
            assert_eq!(device.buffer_size, before);
        }
        for (property, address) in [(0, 0x5000), (2, 0x5000), (1, 0), (1, 0x5000), (1, 0x1ff8)] {
            words(&mut memory, 0x1ff8, &[20, 16]);
            assert!(!matches!(
                device.set_property(property, address, &memory, &desktop),
                Ok(0)
            ));
            assert_eq!(device.buffer_size, before);
        }
        words(&mut memory, 0x1301, &[20, 16, 0, 0, 32]);
        assert_eq!(
            device.set_property(1, 0x1301, &memory, &desktop).ok(),
            Some(0)
        );
        assert_eq!(
            device.buffer_size,
            BufferSize {
                requested: 32,
                capacity: 32
            }
        );
        assert_eq!(device.references, 1);
        assert_eq!(device.format, None);
        assert_eq!(device.cooperative_window, None);
        assert_eq!(memory.mapped_pages(), 1);
    }

    #[test]
    fn cooperative_window_is_independent_of_format_refs_and_other_mice() {
        let desktop = desktop();
        let activation = desktop.activation();
        let mut first = Device::new();
        let mut second = Device::new();
        assert_eq!(first.cooperative_window, None);
        assert_eq!(second.cooperative_window, None);
        assert_eq!(first.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        assert_eq!(second.set_cooperative_level(8, 5, &desktop).ok(), Some(0));
        assert_eq!(first.format, None);
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        standard(&mut memory);
        assert_eq!(first.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        assert_eq!(first.cooperative_window, Some(4));
        assert_eq!(first.set_cooperative_level(8, 5, &desktop).ok(), Some(0));
        words(&mut memory, 0x1301, &[44]);
        assert_eq!(capabilities(0x1301, &mut memory).ok(), Some(0));
        assert_eq!(first.cooperative_window, Some(8));
        assert_eq!(first.format, Some(Format::StandardMouse2));
        assert_eq!(second.cooperative_window, Some(8));
        assert_eq!(second.format, None);
        assert_eq!((first.references, second.references), (1, 1));
        assert_eq!(desktop.activation(), activation);
        assert_eq!(memory.mapped_pages(), 1);
    }

    #[test]
    fn bad_replacement_keeps_policy_and_removed_windows_are_not_pinned() {
        let mut desktop = desktop();
        let mut device = Device::new();
        assert_eq!(device.set_cooperative_level(4, 5, &desktop).ok(), Some(0));
        for flags in [0, 3, 12, u32::MAX] {
            assert_eq!(
                device.set_cooperative_level(0, flags, &desktop).ok(),
                Some(INVALID_ARGUMENT)
            );
            assert_eq!(device.cooperative_window, Some(4));
        }
        for flags in [6, 9, 10, 0x15, 0x8000_0005] {
            assert!(matches!(
                device.set_cooperative_level(0, flags, &desktop),
                Err(DispatchError::Unsupported)
            ));
            assert_eq!(device.cooperative_window, Some(4));
        }
        for window in [0, 1, 12, 16, u32::MAX] {
            assert_eq!(
                device.set_cooperative_level(window, 5, &desktop).ok(),
                Some(0x8007_0006)
            );
            assert_eq!(device.cooperative_window, Some(4));
        }
        desktop.remove(4);
        assert!(desktop.window(4).is_none());
        assert_eq!(
            device.set_cooperative_level(4, 5, &desktop).ok(),
            Some(0x8007_0006)
        );
        assert_eq!(device.cooperative_window, Some(4));
        let activation = desktop.activation();
        assert_eq!(device.set_cooperative_level(8, 5, &desktop).ok(), Some(0));
        assert_eq!(device.cooperative_window, Some(8));
        assert_eq!(desktop.activation(), activation);
        assert!(desktop.window(4).is_none());
        assert_eq!(device.references, 1);
    }

    #[test]
    fn failed_replacements_preserve_owned_format_and_recover_on_same_device() {
        let desktop = desktop();
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        for configured in [false, true] {
            let mut device = Device::new();
            standard(&mut memory);
            if configured {
                assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
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
                let result = device.set_format(0x1001, &memory, &desktop);
                assert!(!matches!(result, Ok(0)));
                assert_eq!(device.format, before);
                assert_eq!(device.references, 1);
            }
            standard(&mut memory);
            assert_eq!(device.set_format(0x1001, &memory, &desktop).ok(), Some(0));
            assert_eq!(device.format, Some(Format::StandardMouse2));
            memory.write(0x1000, &[0; 4096]).unwrap();
            assert_eq!(
                device.set_format(0x1001, &memory, &desktop).ok(),
                Some(INVALID_ARGUMENT)
            );
            assert_eq!(device.format, Some(Format::StandardMouse2));
        }
        let mut first = Device::new();
        let second = Device::new();
        standard(&mut memory);
        assert_eq!(first.set_format(0x1001, &memory, &desktop).ok(), Some(0));
        memory.unmap(0x1000, 4096).unwrap();
        assert!(first.set_format(0x1001, &memory, &desktop).is_err());
        assert_eq!(first.format, Some(Format::StandardMouse2));
        assert_eq!(second.format, None);
        assert_eq!((first.references, second.references), (1, 1));
    }
}
