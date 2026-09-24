use super::desktop::Desktop;
use super::{API_BASE, Access, DispatchError, GuestMemory, PAGE_SIZE, Permissions, guest};

mod buffer;
mod keyboard;
mod mouse;

pub(super) const BASE: u32 = 0x7001_7000;
const TABLE: u32 = BASE + 0x100;
const OBJECTS: u32 = BASE + 0x800;
const MAX_ROOTS: usize = 64;
const DEVICE_TABLE: u32 = BASE + 0x200;
const DEVICES: u32 = BASE + 0x900;
const MAX_DEVICES: usize = 64;
const MOUSE_TABLE: u32 = BASE + 0x300;
const MICE: u32 = BASE + 0xa00;
const MAX_MICE: usize = 64;
const MOUSE: [u8; 16] = [
    0x60, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];
const KEYBOARD: [u8; 16] = [
    0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
];
const NO_INTERFACE: u32 = 0x8000_4002;
const NULL_POINTER: u32 = 0x8000_4003;
const INVALID_ARGUMENT: u32 = 0x8007_0057;
const OUT_OF_MEMORY: u32 = 0x8007_000e;
const INTERFACES: [[u8; 16]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    [
        0x60, 0x13, 0x52, 0x89, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
    ],
    [
        0x62, 0xe6, 0x44, 0x59, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
    ],
    [
        0x84, 0xb6, 0x4c, 0x9a, 0x6d, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0, 0xc0, 0x4f, 0x68, 0x44, 0xae,
    ],
];
const DEVICE_INTERFACES: [[u8; 16]; 4] = [
    INTERFACES[0],
    [
        0x80, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
    ],
    [
        0x82, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
    ],
    [
        0xbc, 0xc6, 0xd7, 0x57, 0x56, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0, 0xc0, 0x4f, 0x68, 0x44, 0xae,
    ],
];

#[derive(Clone, Copy)]
pub(super) enum Class {
    Root,
    Keyboard,
    Mouse,
}

impl Class {
    fn objects(self) -> u32 {
        match self {
            Self::Root => OBJECTS,
            Self::Keyboard => DEVICES,
            Self::Mouse => MICE,
        }
    }

    fn interfaces(self) -> &'static [[u8; 16]; 4] {
        match self {
            Self::Root => &INTERFACES,
            Self::Keyboard | Self::Mouse => &DEVICE_INTERFACES,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Create,
    CreateDevice,
    SetDataFormat,
    SetMouseDataFormat,
    SetMouseCooperativeLevel,
    MouseCapabilities,
    SetMouseProperty,
    GetMouseProperty,
    AcquireMouse,
    UnacquireMouse,
    SetCooperativeLevel,
    SetProperty,
    Acquire,
    Unacquire,
    QueryInterface(Class),
    AddRef(Class),
    Release(Class),
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x558 => Some(Self::Create),
            0x55c => Some(Self::QueryInterface(Class::Root)),
            0x560 => Some(Self::AddRef(Class::Root)),
            0x564 => Some(Self::Release(Class::Root)),
            0x568 => Some(Self::CreateDevice),
            0x56c => Some(Self::QueryInterface(Class::Keyboard)),
            0x570 => Some(Self::AddRef(Class::Keyboard)),
            0x574 => Some(Self::Release(Class::Keyboard)),
            0x578 => Some(Self::SetDataFormat),
            0x57c => Some(Self::SetCooperativeLevel),
            0x580 => Some(Self::SetProperty),
            0x584 => Some(Self::Acquire),
            0x588 => Some(Self::Unacquire),
            0x58c => Some(Self::QueryInterface(Class::Mouse)),
            0x590 => Some(Self::AddRef(Class::Mouse)),
            0x594 => Some(Self::Release(Class::Mouse)),
            0x598 => Some(Self::SetMouseDataFormat),
            0x59c => Some(Self::SetMouseCooperativeLevel),
            0x5a0 => Some(Self::MouseCapabilities),
            0x5a4 => Some(Self::SetMouseProperty),
            0x5a8 => Some(Self::GetMouseProperty),
            0x5ac => Some(Self::AcquireMouse),
            0x5b0 => Some(Self::UnacquireMouse),
            _ => None,
        }
    }

    pub(super) fn resolve(name: &str) -> Option<u32> {
        (name == "DirectInputCreateA").then_some(API_BASE + 0x558)
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Create | Self::CreateDevice => 4,
            Self::QueryInterface(_)
            | Self::SetCooperativeLevel
            | Self::SetMouseCooperativeLevel
            | Self::SetMouseProperty
            | Self::GetMouseProperty
            | Self::SetProperty => 3,
            Self::SetDataFormat | Self::SetMouseDataFormat | Self::MouseCapabilities => 2,
            Self::AddRef(_)
            | Self::Release(_)
            | Self::Acquire
            | Self::Unacquire
            | Self::AcquireMouse
            | Self::UnacquireMouse => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Input {
    roots: Vec<u32>,
    devices: Vec<keyboard::Device>,
    mice: Vec<mouse::Device>,
}

impl Input {
    fn references(&mut self, class: Class, index: usize) -> &mut u32 {
        match class {
            Class::Root => &mut self.roots[index],
            Class::Keyboard => &mut self.devices[index].references,
            Class::Mouse => &mut self.mice[index].references,
        }
    }

    fn object(&self, class: Class, pointer: u32) -> Result<usize, DispatchError> {
        let offset = pointer
            .checked_sub(class.objects())
            .ok_or(DispatchError::Unsupported)?;
        let index = usize::try_from(offset / 4).expect("32-bit object index");
        let count = match class {
            Class::Root => self.roots.get(index),
            Class::Keyboard => self.devices.get(index).map(|device| &device.references),
            Class::Mouse => self.mice.get(index).map(|device| &device.references),
        };
        if !offset.is_multiple_of(4) || count.is_none_or(|count| *count == 0) {
            return Err(DispatchError::Unsupported);
        }
        Ok(index)
    }

    fn initialize(memory: &mut GuestMemory) -> Result<(), DispatchError> {
        let mut bytes = [0; 4096];
        for slot in 0..10 {
            let address = API_BASE
                + match slot {
                    0 => 0x55c_u32,
                    1 => 0x560,
                    2 => 0x564,
                    3 => 0x568,
                    _ => 0xffc,
                };
            bytes[0x100 + slot * 4..0x104 + slot * 4].copy_from_slice(&address.to_le_bytes());
        }
        for index in 0..MAX_ROOTS {
            bytes[0x800 + index * 4..0x804 + index * 4].copy_from_slice(&TABLE.to_le_bytes());
        }
        for slot in 0..29 {
            let address = API_BASE
                + match slot {
                    0 => 0x56c_u32,
                    1 => 0x570,
                    2 => 0x574,
                    6 => 0x580,
                    7 => 0x584,
                    8 => 0x588,
                    11 => 0x578,
                    13 => 0x57c,
                    _ => 0xffc,
                };
            bytes[0x200 + slot * 4..0x204 + slot * 4].copy_from_slice(&address.to_le_bytes());
            let address = API_BASE
                + match slot {
                    0 => 0x58c_u32,
                    1 => 0x590,
                    2 => 0x594,
                    3 => 0x5a0,
                    5 => 0x5a8,
                    6 => 0x5a4,
                    7 => 0x5ac,
                    8 => 0x5b0,
                    11 => 0x598,
                    13 => 0x59c,
                    _ => 0xffc,
                };
            bytes[0x300 + slot * 4..0x304 + slot * 4].copy_from_slice(&address.to_le_bytes());
        }
        for index in 0..MAX_DEVICES {
            bytes[0x900 + index * 4..0x904 + index * 4]
                .copy_from_slice(&DEVICE_TABLE.to_le_bytes());
        }
        for index in 0..MAX_MICE {
            bytes[0xa00 + index * 4..0xa04 + index * 4].copy_from_slice(&MOUSE_TABLE.to_le_bytes());
        }
        memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
        memory.write(u64::from(BASE), &bytes)?;
        memory.protect(u64::from(BASE), PAGE_SIZE, Permissions::READ)?;
        Ok(())
    }

    fn create(&mut self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let output = args[2];
        if output == 0 {
            return Ok(NULL_POINTER);
        }
        guest::check(memory, output, 4, Access::Write)?;
        if args[1] != 0x700 || args[3] != 0 {
            return Err(DispatchError::Unsupported);
        }
        let failure = if args[0] == 0 {
            Some(INVALID_ARGUMENT)
        } else if self.roots.len() == MAX_ROOTS {
            Some(OUT_OF_MEMORY)
        } else {
            None
        };
        if let Some(result) = failure {
            guest::write_word(memory, output, 0)?;
            return Ok(result);
        }
        if self.roots.is_empty() {
            Self::initialize(memory)?;
        }
        let pointer = OBJECTS + u32::try_from(self.roots.len()).expect("bounded root count") * 4;
        guest::write_word(memory, output, pointer)?;
        self.roots.push(1);
        Ok(0)
    }

    fn create_device(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        self.object(Class::Root, args[0])?;
        if args[2] == 0 {
            return Ok(NULL_POINTER);
        }
        guest::check(memory, args[2], 4, Access::Write)?;
        if args[3] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if args[1] == 0 {
            guest::write_word(memory, args[2], 0)?;
            return Ok(NULL_POINTER);
        }
        guest::check(memory, args[1], 16, Access::Read)?;
        let mut guid = [0; 16];
        memory.read(u64::from(args[1]), &mut guid)?;
        let (base, count, limit) = match guid {
            KEYBOARD => (DEVICES, self.devices.len(), MAX_DEVICES),
            MOUSE => (MICE, self.mice.len(), MAX_MICE),
            _ => return Err(DispatchError::Unsupported),
        };
        if count == limit {
            guest::write_word(memory, args[2], 0)?;
            return Ok(OUT_OF_MEMORY);
        }
        let pointer = base + u32::try_from(count).expect("bounded device count") * 4;
        guest::write_word(memory, args[2], pointer)?;
        if guid == KEYBOARD {
            self.devices.push(keyboard::Device::new());
        } else {
            self.mice.push(mouse::Device::new());
        }
        Ok(0)
    }

    fn query(
        &mut self,
        class: Class,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let index = self.object(class, args[0])?;
        if args[1] == 0 || args[2] == 0 {
            return Ok(NULL_POINTER);
        }
        guest::check(memory, args[2], 4, Access::Write)?;
        guest::check(memory, args[1], 16, Access::Read)?;
        let mut iid = [0; 16];
        memory.read(u64::from(args[1]), &mut iid)?;
        if !class.interfaces().contains(&iid) {
            guest::write_word(memory, args[2], 0)?;
            return Ok(NO_INTERFACE);
        }
        let count = self
            .references(class, index)
            .checked_add(1)
            .ok_or(DispatchError::Unsupported)?;
        guest::write_word(memory, args[2], args[0])?;
        *self.references(class, index) = count;
        Ok(0)
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
        desktop: &Desktop,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Create => self.create(args, memory),
            Call::CreateDevice => self.create_device(args, memory),
            Call::SetDataFormat => {
                let index = self.object(Class::Keyboard, args[0])?;
                self.devices[index].set_format(args[1], memory, desktop)
            }
            Call::SetMouseDataFormat => {
                let index = self.object(Class::Mouse, args[0])?;
                self.mice[index].set_format(args[1], memory, desktop)
            }
            Call::SetMouseCooperativeLevel => {
                let index = self.object(Class::Mouse, args[0])?;
                self.mice[index].set_cooperative_level(args[1], args[2], desktop)
            }
            Call::MouseCapabilities => {
                self.object(Class::Mouse, args[0])?;
                mouse::capabilities(args[1], memory)
            }
            Call::SetMouseProperty => {
                let index = self.object(Class::Mouse, args[0])?;
                self.mice[index].set_property(args[1], args[2], memory, desktop)
            }
            Call::GetMouseProperty => {
                let index = self.object(Class::Mouse, args[0])?;
                self.mice[index].get_property(args[1], args[2], memory)
            }
            Call::AcquireMouse => {
                let index = self.object(Class::Mouse, args[0])?;
                let (before, rest) = self.mice.split_at_mut(index);
                let (device, after) = rest.split_first_mut().expect("validated mouse identity");
                device.acquire(desktop, || {
                    before
                        .iter()
                        .chain(after.iter())
                        .any(|other| other.is_acquired(desktop))
                })
            }
            Call::UnacquireMouse => {
                let index = self.object(Class::Mouse, args[0])?;
                Ok(self.mice[index].unacquire(desktop))
            }
            Call::SetCooperativeLevel => {
                let index = self.object(Class::Keyboard, args[0])?;
                self.devices[index].set_cooperative_level(args[1], args[2], desktop)
            }
            Call::SetProperty => {
                let index = self.object(Class::Keyboard, args[0])?;
                self.devices[index].set_property(args[1], args[2], memory, desktop)
            }
            Call::Acquire => {
                let index = self.object(Class::Keyboard, args[0])?;
                self.devices[index].acquire(desktop)
            }
            Call::Unacquire => {
                let index = self.object(Class::Keyboard, args[0])?;
                Ok(self.devices[index].unacquire(desktop))
            }
            Call::QueryInterface(class) => self.query(class, args, memory),
            Call::AddRef(class) | Call::Release(class) => {
                let index = self.object(class, args[0])?;
                let references = self.references(class, index);
                let count = if matches!(call, Call::AddRef(_)) {
                    references
                        .checked_add(1)
                        .ok_or(DispatchError::Unsupported)?
                } else {
                    *references - 1
                };
                *references = count;
                Ok(count)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_retains_count_and_query_output() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1000, &INTERFACES[0]).unwrap();
        for class in [Class::Root, Class::Keyboard, Class::Mouse] {
            guest::write_word(&mut memory, 0x1020, 77).unwrap();
            let mut input = Input::default();
            input.roots.push(u32::MAX);
            input.devices.push(keyboard::Device::new());
            input.mice.push(mouse::Device::new());
            *input.references(class, 0) = u32::MAX;
            let pointer = class.objects();
            assert!(matches!(
                input.dispatch(
                    Call::AddRef(class),
                    &[pointer],
                    &mut memory,
                    &Desktop::default()
                ),
                Err(DispatchError::Unsupported)
            ));
            assert!(matches!(
                input.dispatch(
                    Call::QueryInterface(class),
                    &[pointer, 0x1000, 0x1020],
                    &mut memory,
                    &Desktop::default()
                ),
                Err(DispatchError::Unsupported)
            ));
            let mut output = [0];
            guest::read_words(&memory, 0x1020, &mut output).unwrap();
            assert_eq!(output, [77]);
            assert_eq!(*input.references(class, 0), u32::MAX);
            assert_eq!(
                input
                    .dispatch(
                        Call::Release(class),
                        &[pointer],
                        &mut memory,
                        &Desktop::default()
                    )
                    .ok()
                    .unwrap(),
                u32::MAX - 1
            );
        }
    }

    #[test]
    fn mapping_failures_can_be_repaired_without_consuming_identity() {
        for collision in [false, true] {
            let mut memory = GuestMemory::new(if collision { 3 } else { 2 });
            memory
                .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            let extra = if collision { u64::from(BASE) } else { 0x2000 };
            memory
                .map_zeroed(extra, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            guest::write_word(&mut memory, 0x1000, 77).unwrap();
            let mut input = Input::default();
            let args = [1, 0x700, 0x1000, 0];
            assert!(
                input
                    .dispatch(Call::Create, &args, &mut memory, &Desktop::default())
                    .is_err()
            );
            assert!(input.roots.is_empty());
            let mut output = [0];
            guest::read_words(&memory, 0x1000, &mut output).unwrap();
            assert_eq!(output, [77]);
            memory.unmap(extra, PAGE_SIZE).unwrap();
            assert_eq!(
                input
                    .dispatch(Call::Create, &args, &mut memory, &Desktop::default())
                    .ok()
                    .unwrap(),
                0
            );
            guest::read_words(&memory, 0x1000, &mut output).unwrap();
            assert_eq!(output, [OBJECTS]);
            assert_eq!(input.roots, [1]);
        }
    }
}
