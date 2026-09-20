use super::{API_BASE, Cpu32, GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

mod floating;
mod initializers;

const DATA: u32 = 0x7000_2000;
const FMODE: u32 = DATA;
const COMMODE: u32 = DATA + 4;
const ADJUST_FDIV: u32 = DATA + 8;
const ACMDLN: u32 = DATA + 12;
const ARGC: u32 = DATA + 16;
const ARGV: u32 = DATA + 20;
const ENVIRON: u32 = DATA + 24;
const INITENV: u32 = DATA + 28;

#[derive(Clone, Copy)]
pub(super) enum Call {
    SetAppType,
    FmodePointer,
    CommodePointer,
    ControlFp,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x100 => Some(Self::SetAppType),
            0x104 => Some(Self::FmodePointer),
            0x108 => Some(Self::CommodePointer),
            0x10c => Some(Self::ControlFp),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::SetAppType => 1,
            Self::ControlFp => 2,
            Self::FmodePointer | Self::CommodePointer => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Crt {
    pub(super) application_type: i32,
}

impl Crt {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        cpu: &mut Cpu32,
    ) -> Option<u32> {
        match call {
            Call::SetAppType => {
                self.application_type = arguments[0].cast_signed();
                None
            }
            Call::FmodePointer => Some(FMODE),
            Call::CommodePointer => Some(COMMODE),
            Call::ControlFp => Some(floating::control(cpu, arguments[0], arguments[1])),
        }
    }
}

pub(super) fn resolve(name: &str) -> Option<u32> {
    match name {
        "__set_app_type" => Some(API_BASE + 0x100),
        "__p__fmode" => Some(API_BASE + 0x104),
        "__p__commode" => Some(API_BASE + 0x108),
        "_controlfp" => Some(API_BASE + 0x10c),
        "_initterm" => Some(initializers::BASE),
        "_fmode" => Some(FMODE),
        "_commode" => Some(COMMODE),
        "_adjust_fdiv" => Some(ADJUST_FDIV),
        "_acmdln" => Some(ACMDLN),
        "__argc" => Some(ARGC),
        "__argv" => Some(ARGV),
        "_environ" => Some(ENVIRON),
        "__initenv" => Some(INITENV),
        _ => None,
    }
}

pub(super) fn initialize(
    memory: &mut GuestMemory,
    parameters: &super::parameters::Parameters,
) -> Result<(), MemoryError> {
    memory.map_zeroed(u64::from(DATA), PAGE_SIZE, Permissions::READ_WRITE)?;
    guest::write_word(memory, FMODE, 0x4000)?;
    for (address, value) in [
        (ACMDLN, parameters.command_line),
        (ARGC, parameters.argc),
        (ARGV, parameters.argv),
        (ENVIRON, parameters.environment),
    ] {
        guest::write_word(memory, address, value)?;
    }
    initializers::initialize(memory)
}
