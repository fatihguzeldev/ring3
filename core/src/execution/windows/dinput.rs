use super::{API_BASE, Access, DispatchError, GuestMemory, PAGE_SIZE, Permissions, guest};

pub(super) const BASE: u32 = 0x7001_7000;
const TABLE: u32 = BASE + 0x100;
const OBJECTS: u32 = BASE + 0x800;
const MAX_ROOTS: usize = 64;
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

#[derive(Clone, Copy)]
pub(super) enum Call {
    Create,
    QueryInterface,
    AddRef,
    Release,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x558 => Some(Self::Create),
            0x55c => Some(Self::QueryInterface),
            0x560 => Some(Self::AddRef),
            0x564 => Some(Self::Release),
            _ => None,
        }
    }

    pub(super) fn resolve(name: &str) -> Option<u32> {
        (name == "DirectInputCreateA").then_some(API_BASE + 0x558)
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Create => 4,
            Self::QueryInterface => 3,
            Self::AddRef | Self::Release => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Input {
    roots: Vec<u32>,
}

impl Input {
    fn object(&self, pointer: u32) -> Result<usize, DispatchError> {
        let offset = pointer
            .checked_sub(OBJECTS)
            .ok_or(DispatchError::Unsupported)?;
        let index = usize::try_from(offset / 4).expect("32-bit object index");
        if !offset.is_multiple_of(4) || self.roots.get(index).is_none_or(|count| *count == 0) {
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
                    _ => 0xffc,
                };
            bytes[0x100 + slot * 4..0x104 + slot * 4].copy_from_slice(&address.to_le_bytes());
        }
        for index in 0..MAX_ROOTS {
            bytes[0x800 + index * 4..0x804 + index * 4].copy_from_slice(&TABLE.to_le_bytes());
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

    fn query(&mut self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let index = self.object(args[0])?;
        if args[1] == 0 || args[2] == 0 {
            return Ok(NULL_POINTER);
        }
        guest::check(memory, args[2], 4, Access::Write)?;
        guest::check(memory, args[1], 16, Access::Read)?;
        let mut iid = [0; 16];
        memory.read(u64::from(args[1]), &mut iid)?;
        if !INTERFACES.contains(&iid) {
            guest::write_word(memory, args[2], 0)?;
            return Ok(NO_INTERFACE);
        }
        let count = self.roots[index]
            .checked_add(1)
            .ok_or(DispatchError::Unsupported)?;
        guest::write_word(memory, args[2], args[0])?;
        self.roots[index] = count;
        Ok(0)
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Create => self.create(args, memory),
            Call::QueryInterface => self.query(args, memory),
            Call::AddRef | Call::Release => {
                let index = self.object(args[0])?;
                let count = if matches!(call, Call::AddRef) {
                    self.roots[index]
                        .checked_add(1)
                        .ok_or(DispatchError::Unsupported)?
                } else {
                    self.roots[index] - 1
                };
                self.roots[index] = count;
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
        guest::write_word(&mut memory, 0x1020, 77).unwrap();
        let mut input = Input {
            roots: vec![u32::MAX],
        };
        assert!(matches!(
            input.dispatch(Call::AddRef, &[OBJECTS], &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert!(matches!(
            input.dispatch(
                Call::QueryInterface,
                &[OBJECTS, 0x1000, 0x1020],
                &mut memory
            ),
            Err(DispatchError::Unsupported)
        ));
        let mut output = [0];
        guest::read_words(&memory, 0x1020, &mut output).unwrap();
        assert_eq!(output, [77]);
        assert_eq!(input.roots, [u32::MAX]);
        assert_eq!(
            input
                .dispatch(Call::Release, &[OBJECTS], &mut memory)
                .ok()
                .unwrap(),
            u32::MAX - 1
        );
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
            assert!(input.dispatch(Call::Create, &args, &mut memory).is_err());
            assert!(input.roots.is_empty());
            let mut output = [0];
            guest::read_words(&memory, 0x1000, &mut output).unwrap();
            assert_eq!(output, [77]);
            memory.unmap(extra, PAGE_SIZE).unwrap();
            assert_eq!(
                input
                    .dispatch(Call::Create, &args, &mut memory)
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
