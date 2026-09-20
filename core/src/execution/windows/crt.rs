use super::super::Access;
use super::{
    API_BASE, Cpu32, DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32,
    guest, heap,
};

mod arguments;
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
const ERRNO: u32 = DATA + 32;

#[derive(Clone, Copy)]
pub(super) enum Call {
    SetAppType,
    FmodePointer,
    CommodePointer,
    ControlFp,
    GetMainArgs,
    Memset,
    Malloc,
    Free,
    ErrnoPointer,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x100 => Some(Self::SetAppType),
            0x104 => Some(Self::FmodePointer),
            0x108 => Some(Self::CommodePointer),
            0x10c => Some(Self::ControlFp),
            0x110 => Some(Self::GetMainArgs),
            0x114 => Some(Self::Memset),
            0x11c => Some(Self::Malloc),
            0x120 => Some(Self::Free),
            0x124 => Some(Self::ErrnoPointer),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::SetAppType | Self::Malloc | Self::Free => 1,
            Self::ControlFp => 2,
            Self::GetMainArgs => 5,
            Self::Memset => 3,
            Self::FmodePointer | Self::CommodePointer | Self::ErrnoPointer => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Crt {
    pub(super) application_type: i32,
    pub(super) new_mode: u32,
}

impl Crt {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        heap: &mut heap::Heap,
    ) -> Result<Option<u32>, DispatchError> {
        Ok(match call {
            Call::SetAppType => {
                self.application_type = arguments[0].cast_signed();
                None
            }
            Call::Malloc => Some(
                if let Some(pointer) = heap.allocate_crt(arguments[0], memory)? {
                    pointer
                } else {
                    guest::write_word(memory, ERRNO, 12)?;
                    0
                },
            ),
            Call::Free => {
                heap.free_crt(arguments[0], cpu.register(Register32::Esp), memory)?;
                None
            }
            Call::ErrnoPointer => Some(ERRNO),
            Call::FmodePointer => Some(FMODE),
            Call::CommodePointer => Some(COMMODE),
            Call::ControlFp => Some(floating::control(cpu, arguments[0], arguments[1])),
            Call::GetMainArgs => {
                arguments::get_main(self, arguments, memory)?;
                Some(0)
            }
            Call::Memset => {
                let length = usize::try_from(arguments[2]).expect("u32 count fits target usize");
                guest::check(memory, arguments[0], length, Access::Write)?;
                memory.fill(
                    u64::from(arguments[0]),
                    length,
                    arguments[1].to_le_bytes()[0],
                )?;
                Some(arguments[0])
            }
        })
    }
}

pub(super) fn resolve(name: &str) -> Option<u32> {
    match name {
        "__set_app_type" => Some(API_BASE + 0x100),
        "__p__fmode" => Some(API_BASE + 0x104),
        "__p__commode" => Some(API_BASE + 0x108),
        "_controlfp" => Some(API_BASE + 0x10c),
        "_initterm" => Some(initializers::BASE),
        "__getmainargs" => Some(API_BASE + 0x110),
        "memset" => Some(API_BASE + 0x114),
        "_EH_prolog" => Some(API_BASE + 0x118),
        "malloc" => Some(API_BASE + 0x11c),
        "free" => Some(API_BASE + 0x120),
        "_errno" => Some(API_BASE + 0x124),
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

pub(super) fn enter_exception_frame(
    cpu: &mut Cpu32,
    memory: &mut GuestMemory,
    target: u32,
) -> Result<(), DispatchError> {
    let stack = cpu.register(Register32::Esp);
    let start = stack.checked_sub(16).ok_or(MemoryError::AddressOverflow)?;
    let chain = cpu.fs_base();
    if u64::from(start) < u64::from(chain) + 4 && u64::from(chain) < u64::from(start) + 20 {
        return Err(DispatchError::Unsupported);
    }
    let mut previous = [0];
    guest::read_words(memory, chain, &mut previous)?;
    guest::check(memory, start, 20, Access::Write)?;
    guest::check(memory, chain, 4, Access::Write)?;
    // include the temporary return push below the surviving exception record.
    let values = [
        target,
        previous[0],
        cpu.register(Register32::Eax),
        u32::MAX,
        cpu.register(Register32::Ebp),
    ];
    let mut bytes = [0; 20];
    for (slot, value) in bytes.chunks_exact_mut(4).zip(values) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    memory.write(u64::from(start), &bytes)?;
    guest::write_word(memory, chain, stack - 12)?;
    cpu.set_register(Register32::Eax, target);
    cpu.set_register(Register32::Ebp, stack);
    cpu.set_register(Register32::Esp, stack - 12);
    cpu.eip = target;
    Ok(())
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
