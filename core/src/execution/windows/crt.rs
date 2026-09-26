use super::super::Access;
use super::{
    API_BASE, Cpu32, DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32,
    directory, formatting, guest, heap,
};

mod arguments;
mod buffers;
mod environment;
mod floating;
mod initializers;
mod jump;
mod locals;
mod multibyte;
mod onexit;
mod paths;
mod random;
mod rtti;
mod scanning;
mod sorting;
mod sorting_code;
mod status;
mod streams;
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
    BeginThreadEx,
    SetAppType,
    FmodePointer,
    CommodePointer,
    ArgcPointer,
    ArgvPointer,
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
    FindByte,
    Copy,
    Move,
    CopyString,
    AppendString,
    SplitPath,
    Stream(streams::Call),
    FindCharacter,
    Stat,
    Remove,
    SeedRandom,
    Random,
    FloatToInteger,
    TypeName,
    CompareStringPrefix,
    CompareIgnoringCase,
    Format,
    Sprintf,
    Snprintf,
    Sscanf,
    Lowercase,
    UppercaseCharacter,
    IsSpace,
    LowercaseString,
    UppercaseString,
    SetMbCodePage,
    OnExit,
    DynamicCast,
    Sort,
    Floor,
    InlineMath(floating::InlineMath),
    ComparePrefixIgnoringCase,
    SetJump,
    GetEnvironment,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x548 => Some(Self::BeginThreadEx),
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
            0x17c => Some(Self::Lowercase),
            0x180 => Some(Self::ArgcPointer),
            0x184 => Some(Self::ArgvPointer),
            0x188 => Some(Self::Remove),
            0x18c => Some(Self::AppendString),
            0x190 => Some(Self::SplitPath),
            0x194 => Some(Self::Stream(streams::Call::Open)),
            0x198 => Some(Self::Stream(streams::Call::Read)),
            0x19c => Some(Self::Stream(streams::Call::Close)),
            0x1a0 => Some(Self::Stream(streams::Call::Seek)),
            0x1a4 => Some(Self::Stream(streams::Call::Tell)),
            0x1a8 => Some(Self::UppercaseString),
            0x1c0 => Some(Self::LowercaseString),
            0x1c4 => Some(Self::UppercaseCharacter),
            0x1c8 => Some(Self::FindByte),
            0x1cc => Some(Self::Sort),
            0x1d0 => Some(Self::Floor),
            0x1d4 => Some(Self::ComparePrefixIgnoringCase),
            0x1d8 => Some(Self::SetJump),
            0x1dc => Some(Self::GetEnvironment),
            0x1e0 => Some(Self::IsSpace),
            0x1e4 => Some(Self::InlineMath(floating::InlineMath::Fmod)),
            0x1e8 => Some(Self::InlineMath(floating::InlineMath::Asin)),
            0x1ec => Some(Self::InlineMath(floating::InlineMath::Acos)),
            0x1f0 => Some(Self::InlineMath(floating::InlineMath::Pow)),
            0x1ac => Some(Self::Sprintf),
            0x1bc => Some(Self::Snprintf),
            0x1b0 => Some(Self::Move),
            0x1b4 => Some(Self::DynamicCast),
            0x1b8 => Some(Self::Sscanf),
            0x13c => Some(Self::SetMbCodePage),
            0x140 => Some(Self::OnExit),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::BeginThreadEx => 6,
            Self::Stream(call) => call.arguments(),
            Self::SetAppType
            | Self::Malloc
            | Self::Free
            | Self::OperatorNew
            | Self::OperatorDelete
            | Self::MbIncrement
            | Self::Duplicate
            | Self::Length
            | Self::SeedRandom
            | Self::Lowercase
            | Self::UppercaseCharacter
            | Self::IsSpace
            | Self::LowercaseString
            | Self::UppercaseString
            | Self::SetMbCodePage
            | Self::OnExit
            | Self::GetEnvironment
            | Self::Remove => 1,
            Self::ControlFp
            | Self::Floor
            | Self::SetJump
            | Self::MbSearchReverse
            | Self::FindCharacter
            | Self::Stat
            | Self::CompareIgnoringCase
            | Self::Sprintf
            | Self::Sscanf => 2,
            Self::GetMainArgs | Self::SplitPath | Self::DynamicCast => 5,
            Self::Format | Self::Sort => 4,
            Self::Snprintf
            | Self::Memset
            | Self::DllOnExit
            | Self::Compare
            | Self::FindByte
            | Self::Copy
            | Self::Move
            | Self::CopyString
            | Self::AppendString
            | Self::CompareStringPrefix
            | Self::ComparePrefixIgnoringCase => 3,
            Self::FmodePointer
            | Self::CommodePointer
            | Self::ArgcPointer
            | Self::ArgvPointer
            | Self::ErrnoPointer
            | Self::Random
            | Self::FloatToInteger
            | Self::InlineMath(_)
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
    locals: locals::Locals,
    streams: streams::Streams,
}

impl super::Process32 {
    pub(super) fn sort(&mut self, arguments: &[u32]) -> Result<bool, DispatchError> {
        sorting::start(&mut self.cpu, &self.memory, arguments)
    }

    pub(super) fn crt_call(&mut self, call: Call, arguments: &[u32]) -> Result<(), DispatchError> {
        if matches!(call, Call::BeginThreadEx) {
            let (value, teb) = self.threads.create_suspended(
                arguments,
                &mut self.memory,
                &mut self.sync_objects,
            )?;
            self.tls.register(teb);
            self.crt.locals.register(teb);
            self.cpu.set_register(Register32::Eax, value);
            return Ok(());
        }
        if let Some(value) = self.crt.dispatch(
            call,
            arguments,
            &mut self.cpu,
            &mut self.memory,
            &mut self.heap,
            &mut self.current_directory,
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
        args: &[u32],
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        heap: &mut heap::Heap,
        directory: &mut directory::Directory,
    ) -> Result<Option<u32>, DispatchError> {
        let teb = cpu.fs_base();
        let stack = cpu.register(Register32::Esp);
        let errno = || self.locals.errno(teb);
        Ok(match call {
            Call::BeginThreadEx | Call::Sort => unreachable!("call entry is owned by the process"),
            Call::Stream(call) => self
                .streams
                .dispatch(call, args, cpu, memory, heap, directory, errno()?)
                .map(Some)?,
            Call::SetAppType => {
                self.application_type = args[0].cast_signed();
                None
            }
            Call::Malloc => Some(malloc(memory, heap, args[0], errno()?)?),
            Call::OperatorNew => Some(heap.allocate_crt(args[0], memory)?.unwrap_or(0)),
            Call::Free | Call::OperatorDelete => {
                heap.free_crt(args[0], stack, memory)?;
                None
            }
            Call::ErrnoPointer => Some(errno()?),
            Call::SeedRandom => {
                self.locals.random(cpu.fs_base())?.seed(args[0]);
                None
            }
            Call::Random => Some(self.locals.random(cpu.fs_base())?.next()),
            Call::FloatToInteger => Some(floating::to_integer(cpu)?),
            Call::Floor => floating::floor(cpu, args).map(|()| None)?,
            Call::InlineMath(math) => floating::inline_math(cpu, math).map(|()| None)?,
            Call::SetJump => Some(jump::capture(cpu, memory, args)?),
            Call::GetEnvironment => Some(environment::get(memory, args[0])?),
            Call::TypeName => Some(type_names::name(
                memory,
                heap,
                cpu.register(Register32::Ecx),
                errno()?,
            )?),
            Call::DynamicCast => Some(rtti::dynamic_cast(memory, args)?),
            Call::MbSearchReverse => Some(self.multibyte.reverse_search(memory, args[0], args[1])?),
            Call::MbIncrement => Some(self.multibyte.increment(memory, args[0])?),
            Call::SetMbCodePage => {
                self.multibyte.set(args[0].cast_signed())?;
                Some(0)
            }
            Call::OnExit => Some(self.exit_callbacks.register(args[0])),
            Call::Duplicate => Some(strings::duplicate(memory, heap, args[0], errno()?)?),
            Call::Length => Some(strings::length(memory, args[0])?),
            Call::Format => Some(formatting::write(memory, args)?),
            Call::Sprintf => Some(sprintf(memory, args, stack, errno()?)?),
            Call::Snprintf => Some(snprintf(memory, args, stack, errno()?)?),
            Call::Sscanf => Some(scanning::sscanf(memory, args, stack, errno()?)?),
            Call::Lowercase => Some(strings::lowercase(args[0])?),
            Call::UppercaseCharacter => Some(strings::uppercase_character(args[0])?),
            Call::IsSpace => Some(strings::is_space(args[0])?),
            Call::LowercaseString => Some(strings::lowercase_string(memory, args[0], errno()?)?),
            Call::UppercaseString => Some(strings::uppercase(memory, args[0], errno()?)?),
            Call::CompareIgnoringCase => {
                Some(strings::compare_ignoring_case(memory, args[0], args[1])?)
            }
            Call::CompareStringPrefix => {
                Some(strings::compare_prefix(memory, args[0], args[1], args[2])?)
            }
            Call::ComparePrefixIgnoringCase => Some(strings::compare_prefix_ignoring_case(
                memory, args[0], args[1], args[2],
            )?),
            Call::FindCharacter => Some(strings::find(memory, args[0], args[1])?),
            Call::SplitPath => {
                self.multibyte.split_path(memory, args)?;
                None
            }
            Call::Remove => Some(status::remove(directory, memory, args[0], errno()?)?),
            Call::Stat => Some(status::query(
                directory,
                memory,
                args[0],
                args[1],
                errno()?,
            )?),
            Call::Copy | Call::CopyString | Call::AppendString => {
                let copy = match call {
                    Call::CopyString => strings::copy,
                    Call::AppendString => strings::append,
                    _ => buffers::copy,
                };
                Some(copy(memory, args[0], args[1], args[2])?)
            }
            Call::Move => Some(buffers::move_bytes(memory, args[0], args[1], args[2])?),
            Call::Compare => Some(buffers::compare(memory, args[0], args[1], args[2])?),
            Call::FindByte => Some(buffers::find(memory, args[0], args[1], args[2])?),
            Call::DllOnExit => Some(onexit::register(heap, memory, stack, args, errno()?)?),
            Call::FmodePointer => Some(FMODE),
            Call::CommodePointer => Some(COMMODE),
            Call::ArgcPointer => Some(ARGC),
            Call::ArgvPointer => Some(ARGV),
            Call::ControlFp => Some(floating::control(cpu, args[0], args[1])),
            Call::GetMainArgs => {
                arguments::get_main(self, args, memory)?;
                Some(0)
            }
            Call::Memset => Some(buffers::fill(memory, args[0], args[1], args[2])?),
        })
    }
}

fn sprintf(
    memory: &mut GuestMemory,
    args: &[u32],
    stack: u32,
    errno: u32,
) -> Result<u32, DispatchError> {
    if args[0] == 0 || args[1] == 0 {
        guest::write_word(memory, errno, 22)?;
        Ok(u32::MAX)
    } else {
        formatting::write_variadic(memory, args, stack)
    }
}

fn snprintf(
    memory: &mut GuestMemory,
    args: &[u32],
    stack: u32,
    errno: u32,
) -> Result<u32, DispatchError> {
    if args[0] == 0 || args[2] == 0 {
        guest::write_word(memory, errno, 22)?;
        Ok(u32::MAX)
    } else {
        formatting::write_snprintf_variadic(memory, args, stack)
    }
}

fn malloc(
    memory: &mut GuestMemory,
    heap: &mut heap::Heap,
    size: u32,
    errno: u32,
) -> Result<u32, DispatchError> {
    if let Some(pointer) = heap.allocate_crt(size, memory)? {
        Ok(pointer)
    } else {
        guest::write_word(memory, errno, 12)?;
        Ok(0)
    }
}

pub(super) fn resolve(name: &str) -> Option<u32> {
    match name {
        "__set_app_type" => Some(API_BASE + 0x100),
        "__p__fmode" => Some(API_BASE + 0x104),
        "__p__commode" => Some(API_BASE + 0x108),
        "__p___argc" => Some(API_BASE + 0x180),
        "__p___argv" => Some(API_BASE + 0x184),
        "_controlfp" => Some(API_BASE + 0x10c),
        "_initterm" => Some(initializers::BASE),
        "qsort" => Some(API_BASE + 0x1cc),
        "floor" => Some(API_BASE + 0x1d0),
        "_CIfmod" => Some(API_BASE + 0x1e4),
        "_CIasin" => Some(API_BASE + 0x1e8),
        "_CIacos" => Some(API_BASE + 0x1ec),
        "_CIpow" => Some(API_BASE + 0x1f0),
        "_setjmp3" => Some(API_BASE + 0x1d8),
        "getenv" => Some(API_BASE + 0x1dc),
        "__getmainargs" => Some(API_BASE + 0x110),
        "memset" => Some(API_BASE + 0x114),
        "memcmp" => Some(API_BASE + 0x138),
        "memchr" => Some(API_BASE + 0x1c8),
        "memcpy" => Some(API_BASE + 0x150),
        "strncpy" => Some(API_BASE + 0x154),
        "strncat" => Some(API_BASE + 0x18c),
        "_splitpath" => Some(API_BASE + 0x190),
        "fopen" => Some(API_BASE + 0x194),
        "fread" => Some(API_BASE + 0x198),
        "fclose" => Some(API_BASE + 0x19c),
        "fseek" => Some(API_BASE + 0x1a0),
        "ftell" => Some(API_BASE + 0x1a4),
        "_strupr" => Some(API_BASE + 0x1a8),
        "_strlwr" => Some(API_BASE + 0x1c0),
        "sprintf" => Some(API_BASE + 0x1ac),
        "_snprintf" => Some(API_BASE + 0x1bc),
        "sscanf" => Some(API_BASE + 0x1b8),
        "memmove" => Some(API_BASE + 0x1b0),
        "strchr" => Some(API_BASE + 0x158),
        "_setmbcp" => Some(API_BASE + 0x13c),
        "_onexit" => Some(API_BASE + 0x140),
        "_EH_prolog" => Some(API_BASE + 0x118),
        "_CxxThrowException" => Some(API_BASE + 0x490),
        "__CxxFrameHandler" => Some(API_BASE + 0x494),
        "malloc" => Some(API_BASE + 0x11c),
        "free" => Some(API_BASE + 0x120),
        "??2@YAPAXI@Z" => Some(API_BASE + 0x144),
        "??3@YAXPAX@Z" => Some(API_BASE + 0x148),
        "_errno" => Some(API_BASE + 0x124),
        "_beginthreadex" => Some(API_BASE + 0x548),
        "_stat" => Some(API_BASE + 0x15c),
        "remove" => Some(API_BASE + 0x188),
        "srand" => Some(API_BASE + 0x160),
        "rand" => Some(API_BASE + 0x164),
        "_ftol" => Some(API_BASE + 0x168),
        "?name@type_info@@QBEPBDXZ" => Some(API_BASE + 0x16c),
        "__RTDynamicCast" => Some(API_BASE + 0x1b4),
        "strncmp" => Some(API_BASE + 0x170),
        "_stricmp" => Some(API_BASE + 0x174),
        "_strnicmp" => Some(API_BASE + 0x1d4),
        "_vsnprintf" => Some(API_BASE + 0x178),
        "tolower" => Some(API_BASE + 0x17c),
        "toupper" => Some(API_BASE + 0x1c4),
        "isspace" => Some(API_BASE + 0x1e0),
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
