use super::{API_BASE, GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

const DATA: u32 = 0x7000_2000;
const FMODE: u32 = DATA;
const COMMODE: u32 = DATA + 4;
const ADJUST_FDIV: u32 = DATA + 8;

#[derive(Clone, Copy)]
pub(super) enum Call {
    SetAppType,
    FmodePointer,
    CommodePointer,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x100 => Some(Self::SetAppType),
            0x104 => Some(Self::FmodePointer),
            0x108 => Some(Self::CommodePointer),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        usize::from(matches!(self, Self::SetAppType))
    }
}

#[derive(Default)]
pub(super) struct Crt {
    pub(super) application_type: i32,
}

impl Crt {
    pub(super) fn dispatch(&mut self, call: Call, argument: u32) -> Option<u32> {
        match call {
            Call::SetAppType => {
                self.application_type = argument.cast_signed();
                None
            }
            Call::FmodePointer => Some(FMODE),
            Call::CommodePointer => Some(COMMODE),
        }
    }
}

pub(super) fn resolve(name: &str) -> Option<u32> {
    match name {
        "__set_app_type" => Some(API_BASE + 0x100),
        "__p__fmode" => Some(API_BASE + 0x104),
        "__p__commode" => Some(API_BASE + 0x108),
        "_fmode" => Some(FMODE),
        "_commode" => Some(COMMODE),
        "_adjust_fdiv" => Some(ADJUST_FDIV),
        _ => None,
    }
}

pub(super) fn initialize(memory: &mut GuestMemory) -> Result<(), MemoryError> {
    memory.map_zeroed(u64::from(DATA), PAGE_SIZE, Permissions::READ_WRITE)?;
    guest::write_word(memory, FMODE, 0x4000)
}
