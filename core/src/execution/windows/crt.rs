use super::super::Access;
use super::{
    API_BASE, Cpu32, DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32,
    directory, guest, heap,
};

mod arguments;
mod buffers;
mod floating;
mod formatting;
mod initializers;
mod multibyte;
mod onexit;
mod random;
mod status;
mod strings;
mod type_names;

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
const UNGUARDED_READLC_ACTIVE: u32 = DATA + 36;
const SETLC_ACTIVE: u32 = DATA + 40;
const LC_HANDLE: u32 = DATA + 44;
const LC_CODEPAGE: u32 = DATA + 68;
const LC_COLLATE_CP: u32 = DATA + 72;
const MB_CUR_MAX: u32 = DATA + 76;

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
    OperatorNew,
    OperatorDelete,
    ErrnoPointer,
    DllOnExit,
    MbSearchReverse,
    MbIncrement,
    Duplicate,
    Length,
    Compare,
    Copy,
    CopyString,
    FindCharacter,
    Stat,
    SeedRandom,
    Random,
    FloatToInteger,
    TypeName,
    CompareStringPrefix,
    CompareIgnoringCase,
    Format,
    SetMbCodePage,
    OnExit,
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
            0x144 => Some(Self::OperatorNew),
            0x148 => Some(Self::OperatorDelete),
            0x124 => Some(Self::ErrnoPointer),
            0x128 => Some(Self::DllOnExit),
            0x12c => Some(Self::MbSearchReverse),
            0x130 => Some(Self::MbIncrement),
            0x134 => Some(Self::Duplicate),
            0x14c => Some(Self::Length),
            0x138 => Some(Self::Compare),
            0x150 => Some(Self::Copy),
            0x154 => Some(Self::CopyString),
            0x158 => Some(Self::FindCharacter),
            0x15c => Some(Self::Stat),
            0x160 => Some(Self::SeedRandom),
            0x164 => Some(Self::Random),
            0x168 => Some(Self::FloatToInteger),
            0x16c => Some(Self::TypeName),
            0x170 => Some(Self::CompareStringPrefix),
            0x174 => Some(Self::CompareIgnoringCase),
            0x178 => Some(Self::Format),
            0x13c => Some(Self::SetMbCodePage),
            0x140 => Some(Self::OnExit),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::SetAppType
            | Self::Malloc
            | Self::Free
            | Self::OperatorNew
            | Self::OperatorDelete
            | Self::MbIncrement
            | Self::Duplicate
            | Self::Length
            | Self::SeedRandom
            | Self::SetMbCodePage
            | Self::OnExit => 1,
            Self::ControlFp
            | Self::MbSearchReverse
            | Self::FindCharacter
            | Self::Stat
            | Self::CompareIgnoringCase => 2,
            Self::GetMainArgs => 5,
            Self::Format => 4,
            Self::Memset
            | Self::DllOnExit
            | Self::Compare
            | Self::Copy
            | Self::CopyString
            | Self::CompareStringPrefix => 3,
            Self::FmodePointer
            | Self::CommodePointer
            | Self::ErrnoPointer
            | Self::Random
            | Self::FloatToInteger
            | Self::TypeName => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Crt {
    pub(super) application_type: i32,
    pub(super) new_mode: u32,
    multibyte: multibyte::CodePage,
    exit_callbacks: onexit::Registry,
    random: random::Sequence,
}

impl super::Process32 {
    pub(super) fn crt_call(&mut self, call: Call, arguments: &[u32]) -> Result<(), DispatchError> {
        if let Some(value) = self.crt.dispatch(
            call,
            arguments,
            &mut self.cpu,
            &mut self.memory,
            &mut self.heap,
            &self.current_directory,
        )? {
            self.cpu.set_register(Register32::Eax, value);
        }
        Ok(())
    }
}

impl Crt {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        heap: &mut heap::Heap,
        directory: &directory::Directory,
    ) -> Result<Option<u32>, DispatchError> {
        Ok(match call {
            Call::SetAppType => {
                self.application_type = arguments[0].cast_signed();
                None
            }
            Call::Malloc => Some(malloc(memory, heap, arguments[0])?),
            Call::OperatorNew => Some(heap.allocate_crt(arguments[0], memory)?.unwrap_or(0)),
            Call::Free | Call::OperatorDelete => {
                heap.free_crt(arguments[0], cpu.register(Register32::Esp), memory)?;
                None
            }
            Call::ErrnoPointer => Some(ERRNO),
            Call::SeedRandom => {
                self.random.seed(arguments[0]);
                None
            }
            Call::Random => Some(self.random.next()),
            Call::FloatToInteger => Some(floating::to_integer(cpu)?),
            Call::TypeName => Some(type_names::name(
                memory,
                heap,
                cpu.register(Register32::Ecx),
            )?),
            Call::MbSearchReverse => Some(self.multibyte.reverse_search(
                memory,
                arguments[0],
                arguments[1],
            )?),
            Call::MbIncrement => Some(self.multibyte.increment(memory, arguments[0])?),
            Call::SetMbCodePage => {
                self.multibyte.set(arguments[0].cast_signed())?;
                Some(0)
            }
            Call::OnExit => Some(self.exit_callbacks.register(arguments[0])),
            Call::Duplicate => Some(strings::duplicate(memory, heap, arguments[0])?),
            Call::Length => Some(strings::length(memory, arguments[0])?),
            Call::Format => Some(formatting::write(memory, arguments)?),
            Call::CompareIgnoringCase => Some(strings::compare_ignoring_case(
                memory,
                arguments[0],
                arguments[1],
            )?),
            Call::CompareStringPrefix => Some(strings::compare_prefix(
                memory,
                arguments[0],
                arguments[1],
                arguments[2],
            )?),
            Call::FindCharacter => Some(strings::find(memory, arguments[0], arguments[1])?),
            Call::Stat => Some(status::query(
                directory,
                memory,
                arguments[0],
                arguments[1],
            )?),
            Call::CopyString => Some(strings::copy(
                memory,
                arguments[0],
                arguments[1],
                arguments[2],
            )?),
            Call::Copy => Some(buffers::copy(
                memory,
                arguments[0],
                arguments[1],
                arguments[2],
            )?),
            Call::Compare => Some(buffers::compare(
                memory,
                arguments[0],
                arguments[1],
                arguments[2],
            )?),
            Call::DllOnExit => Some(onexit::register(
                heap,
                memory,
                cpu.register(Register32::Esp),
                arguments,
            )?),
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

fn malloc(
    memory: &mut GuestMemory,
    heap: &mut heap::Heap,
    size: u32,
) -> Result<u32, DispatchError> {
    if let Some(pointer) = heap.allocate_crt(size, memory)? {
        Ok(pointer)
    } else {
        guest::write_word(memory, ERRNO, 12)?;
        Ok(0)
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
        "memcmp" => Some(API_BASE + 0x138),
        "memcpy" => Some(API_BASE + 0x150),
        "strncpy" => Some(API_BASE + 0x154),
        "strchr" => Some(API_BASE + 0x158),
        "_setmbcp" => Some(API_BASE + 0x13c),
        "_onexit" => Some(API_BASE + 0x140),
        "_EH_prolog" => Some(API_BASE + 0x118),
        "malloc" => Some(API_BASE + 0x11c),
        "free" => Some(API_BASE + 0x120),
        "??2@YAPAXI@Z" => Some(API_BASE + 0x144),
        "??3@YAXPAX@Z" => Some(API_BASE + 0x148),
        "_errno" => Some(API_BASE + 0x124),
        "_stat" => Some(API_BASE + 0x15c),
        "srand" => Some(API_BASE + 0x160),
        "rand" => Some(API_BASE + 0x164),
        "_ftol" => Some(API_BASE + 0x168),
        "?name@type_info@@QBEPBDXZ" => Some(API_BASE + 0x16c),
        "strncmp" => Some(API_BASE + 0x170),
        "_stricmp" => Some(API_BASE + 0x174),
        "_vsnprintf" => Some(API_BASE + 0x178),
        "__dllonexit" => Some(API_BASE + 0x128),
        "_mbsrchr" => Some(API_BASE + 0x12c),
        "_mbsinc" => Some(API_BASE + 0x130),
        "_strdup" => Some(API_BASE + 0x134),
        "strlen" => Some(API_BASE + 0x14c),
        "_fmode" => Some(FMODE),
        "_commode" => Some(COMMODE),
        "_adjust_fdiv" => Some(ADJUST_FDIV),
        "_acmdln" => Some(ACMDLN),
        "__argc" => Some(ARGC),
        "__argv" => Some(ARGV),
        "_environ" => Some(ENVIRON),
        "__initenv" => Some(INITENV),
        "__unguarded_readlc_active" => Some(UNGUARDED_READLC_ACTIVE),
        "__setlc_active" => Some(SETLC_ACTIVE),
        "__lc_handle" => Some(LC_HANDLE),
        "__lc_codepage" => Some(LC_CODEPAGE),
        "__lc_collate_cp" => Some(LC_COLLATE_CP),
        "__mb_cur_max" => Some(MB_CUR_MAX),
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
        (MB_CUR_MAX, 1),
        (ACMDLN, parameters.command_line),
        (ARGC, parameters.argc),
        (ARGV, parameters.argv),
        (ENVIRON, parameters.environment),
    ] {
        guest::write_word(memory, address, value)?;
    }
    initializers::initialize(memory)
}
