use super::loader::{GuestModule, load_modules};
use super::{
    Access, Cpu32, GuestMemory, LoadError, MemoryError, PAGE_SIZE, Permissions, Register32,
    StopReason,
};
use crate::PeImportSymbol;

mod atomics;
mod callbacks;
mod child_boundary;
mod classes;
mod clock;
mod code_pages;
mod com;
mod creation;
mod critical_sections;
mod crt;
mod cursors;
mod d3d8;
mod desktop;
mod diagnostics;
mod dialogs;
mod dinput;
mod directory;
mod dsound;
mod eh;
mod environment;
mod event_waits;
mod formatting;
mod gdi;
mod guest;
mod heap;
mod hooks;
mod messages;
mod modules;
mod parameters;
mod registry;
mod resources;
mod startup;
mod strings;
mod synchronization;
mod system;
mod thread;
mod threads;
mod tls;
mod user_atoms;

pub use clock::ClockError;
pub use d3d8::Frame;
pub use desktop::WindowSnapshot;
pub use directory::{
    FileContents, FileContentsMode, FileContentsRequest, FileMetadata, SupplyFileContentsError,
};
pub use messages::{PostMessageError, PostedMessage};
pub use parameters::ProcessOptions;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardInputError {
    Exited,
}

const API_BASE: u32 = 0x7000_0000;
const STACK_BASE: u32 = 0x1000_0000;
const STACK_SIZE: u32 = 64 * 1024;
// immutable nt 5.1/build 2600 guest identity, independent of the host and executable.
const GUEST_VERSION: u32 = (2600 << 16) | (1 << 8) | 5;

/// a guest process with bounded thread scheduling and a win32 import boundary.
pub struct Process32 {
    pub memory: GuestMemory,
    pub cpu: Cpu32,
    exit_code: Option<u32>,
    error_mode: u32,
    elapsed_nanoseconds: i64,
    command_line: u32,
    subsystem_version: u32,
    startup: startup::Startup,
    modules: modules::Modules,
    registry: registry::Registry,
    resources: resources::Resources,
    current_directory: directory::Directory,
    environment: environment::Environment,
    user_atoms: user_atoms::UserAtoms,
    classes: classes::Classes,
    desktop: desktop::Desktop,
    messages: messages::Queue,
    cursors: cursors::Cursors,
    heap: heap::Heap,
    critical_sections: critical_sections::CriticalSections,
    sync_objects: synchronization::SyncObjects,
    threads: threads::Threads,
    hooks: hooks::Hooks,
    tls: tls::Tls,
    graphics: d3d8::Graphics,
    sound_data_mapped: bool,
    gdi: gdi::Gdi,
    crt: crt::Crt,
    com: com::Com,
    input: dinput::Input,
    diagnostic_imports: diagnostics::Imports,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessStop {
    Exited(u32),
    /// all guest work is paused until the host supplies the pending file snapshot.
    FileContentsRequired,
    /// the current thread has no queued message and can resume after one arrives.
    WaitingForMessage,
    /// every activated thread is waiting for an event; no guest work is ready.
    WaitingForSynchronization,
    DllInitializationFailed {
        module: String,
    },
    Stopped(StopReason),
    UnsupportedApi {
        address: u32,
    },
    /// an unresolved diagnostic import; ordinal symbols use the `#number` form.
    UnresolvedImport {
        address: u32,
        module: String,
        symbol: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessResult {
    pub reason: ProcessStop,
    /// execution steps; each repeated string element counts as one step.
    pub instructions: u64,
    pub api_calls: u64,
}

#[derive(Clone, Copy)]
enum Api {
    SetLastError,
    GetLastError,
    GetCurrentThread,
    GetCurrentThreadId,
    ResumeThread,
    SuspendThread,
    CloseHandle,
    ExitProcess,
    Interlocked(atomics::Call),
    Clock(clock::Call),
    Directory(directory::Call),
    GetCommandLine,
    GetEnvironmentVariable,
    GetStartupInfo,
    WindowsFormat,
    CallWindowProc,
    SendMessage,
    PostMessage,
    PeekMessage,
    GetMessage,
    TranslateMessage,
    DispatchMessage,
    SetWindowText,
    EnableWindow,
    EndDialog,
    SetWindowPos,
    DestroyWindow,
    ShowWindow,
    UpdateWindow,
    InvalidateRect,
    SetForegroundWindow,
    CreateDialog,
    CallNextHook,
    Window(creation::Call),
    RegisterUserAtom,
    System(system::Call),
    SetErrorMode,
    GetErrorMode,
    GetVersion,
    GetProcessVersion,
    String(strings::Call),
    ExceptionProlog,
    CxxThrow,
    Graphics(d3d8::Call),
    Sound(dsound::Call),
    Gdi(gdi::Call),
    Cursor(cursors::Call),
    Class(classes::Call),
    Desktop(desktop::Call),
    Crt(crt::Call),
    Com(com::Call),
    Input(dinput::Call),
    CodePage(code_pages::Call),
    Module(modules::Call),
    Resource(resources::Call),
    Registry(registry::Call),
    Heap(heap::Call),
    CriticalSection(critical_sections::Call),
    Synchronization(synchronization::Call),
    Hook(hooks::Call),
    ThreadPriority(thread::PriorityCall),
    Tls(tls::Call),
    Unsupported,
}

enum DispatchError {
    Memory(MemoryError),
    Unsupported,
    WaitingForMessage,
    FileContentsRequired,
}

fn collect_modules<'a>(
    startup: &[GuestModule<'a>],
    deferred: &[GuestModule<'a>],
) -> Vec<GuestModule<'a>> {
    startup.iter().chain(deferred).copied().collect()
}

fn reserved_ranges() -> [std::ops::Range<u64>; 9] {
    [
        u64::from(STACK_BASE)..u64::from(STACK_BASE + STACK_SIZE),
        heap::START..heap::END,
        u64::from(API_BASE)..u64::from(startup::BASE) + PAGE_SIZE,
        u64::from(com::BASE)..u64::from(com::BASE) + PAGE_SIZE,
        u64::from(dsound::BASE)..u64::from(dsound::BASE) + PAGE_SIZE,
        u64::from(dinput::BASE)..u64::from(dinput::BASE) + PAGE_SIZE,
        u64::from(diagnostics::BASE)
            ..u64::from(diagnostics::BASE) + u64::from(diagnostics::MAX_IMPORTS) * 4,
        u64::from(thread::BASE)..u64::from(thread::BASE) + PAGE_SIZE,
        u64::from(threads::START)..u64::from(threads::END),
    ]
}

impl DispatchError {
    fn stop(self, address: u32) -> ProcessStop {
        match self {
            Self::Memory(error) => ProcessStop::Stopped(StopReason::MemoryFault(error)),
            Self::Unsupported => ProcessStop::UnsupportedApi { address },
            Self::WaitingForMessage => ProcessStop::WaitingForMessage,
            Self::FileContentsRequired => ProcessStop::FileContentsRequired,
        }
    }
}

impl From<MemoryError> for DispatchError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

impl Api {
    fn at(address: u32) -> Option<Self> {
        match address.checked_sub(API_BASE)? {
            0 => Some(Self::SetLastError),
            4 => Some(Self::GetLastError),
            0x550 => Some(Self::ResumeThread),
            0x554 => Some(Self::SuspendThread),
            0x21c => Some(Self::CloseHandle),
            0xd8 => Some(Self::GetCurrentThread),
            0xdc => Some(Self::GetCurrentThreadId),
            8 => Some(Self::ExitProcess),
            0x200 => Some(Self::Interlocked(atomics::Call::Exchange)),
            0x204 => Some(Self::Interlocked(atomics::Call::Increment)),
            0x208 => Some(Self::Interlocked(atomics::Call::Decrement)),
            0x228 => Some(Self::Directory(directory::Call::Query)),
            0x230 => Some(Self::Directory(directory::Call::Change)),
            0x234 => Some(Self::Directory(directory::Call::FindFirst)),
            0x238 => Some(Self::Directory(directory::Call::FindNext)),
            0x23c => Some(Self::Directory(directory::Call::FindClose)),
            0x290 => Some(Self::Directory(directory::Call::Attributes)),
            0x294 => Some(Self::Directory(directory::Call::ShortPath)),
            0x5b4 => Some(Self::Directory(directory::Call::OpenFile)),
            0x5b8 => Some(Self::Directory(directory::Call::FileSize)),
            0x5bc => Some(Self::Directory(directory::Call::DiskGeometry)),
            0x5c0 => Some(Self::Directory(directory::Call::SeekFile)),
            0x5c4 => Some(Self::Directory(directory::Call::ReadFile)),
            0x240 => Some(Self::GetEnvironmentVariable),
            0x248 => Some(Self::GetStartupInfo),
            0x25c => Some(Self::WindowsFormat),
            0x2a4 => Some(Self::CallWindowProc),
            0x2cc => Some(Self::SendMessage),
            0x464 => Some(Self::PostMessage),
            0x43c => Some(Self::PeekMessage),
            0x448 => Some(Self::GetMessage),
            0x44c => Some(Self::TranslateMessage),
            0x450 => Some(Self::DispatchMessage),
            0x45c => Some(Self::SetWindowText),
            0x460 => Some(Self::EnableWindow),
            0x468 => Some(Self::EndDialog),
            0x46c => Some(Self::SetWindowPos),
            0x470 => Some(Self::DestroyWindow),
            0x440 => Some(Self::ShowWindow),
            0x444 => Some(Self::UpdateWindow),
            0x4b0 => Some(Self::InvalidateRect),
            0x4b4 => Some(Self::SetForegroundWindow),
            0x2c4 => Some(Self::CallNextHook),
            0x22c => Some(Self::GetCommandLine),
            0x434 => Some(Self::CreateDialog),
            0x20 => Some(Self::SetErrorMode),
            0x24 => Some(Self::GetErrorMode),
            0x30 => Some(Self::GetVersion),
            0x94 | 0xc8 => Some(Self::RegisterUserAtom),
            0x98 => Some(Self::GetProcessVersion),
            0x118 => Some(Self::ExceptionProlog),
            0x490 => Some(Self::CxxThrow),
            0x494 | 0x54c | 0xffc => Some(Self::Unsupported),
            offset => d3d8::Call::at(offset)
                .map(Self::Graphics)
                .or_else(|| dsound::Call::at(offset).map(Self::Sound))
                .or_else(|| dinput::Call::at(offset).map(Self::Input))
                .or_else(|| system::Call::at(offset).map(Self::System))
                .or_else(|| creation::Call::at(offset).map(Self::Window))
                .or_else(|| gdi::Call::at(offset).map(Self::Gdi))
                .or_else(|| cursors::Call::at(offset).map(Self::Cursor))
                .or_else(|| classes::Call::at(offset).map(Self::Class))
                .or_else(|| desktop::Call::at(offset).map(Self::Desktop))
                .or_else(|| crt::Call::at(offset).map(Self::Crt))
                .or_else(|| modules::Call::at(offset).map(Self::Module))
                .or_else(|| resources::Call::at(offset).map(Self::Resource))
                .or_else(|| registry::Call::at(offset).map(Self::Registry))
                .or_else(|| strings::Call::at(offset).map(Self::String))
                .or_else(|| heap::Call::at(offset).map(Self::Heap))
                .or_else(|| critical_sections::Call::at(offset).map(Self::CriticalSection))
                .or_else(|| synchronization::Call::at(offset).map(Self::Synchronization))
                .or_else(|| hooks::Call::at(offset).map(Self::Hook))
                .or_else(|| thread::PriorityCall::at(offset).map(Self::ThreadPriority))
                .or_else(|| clock::Call::at(offset).map(Self::Clock))
                .or_else(|| tls::Call::at(offset).map(Self::Tls))
                .or_else(|| code_pages::Call::at(offset).map(Self::CodePage))
                .or_else(|| com::Call::at(offset).map(Self::Com)),
        }
    }

    fn resolve(module: &str, symbol: PeImportSymbol<'_>) -> Option<u32> {
        if module.eq_ignore_ascii_case("dsound.dll") {
            return dsound::Call::resolve(symbol);
        }
        let PeImportSymbol::ByName { name, .. } = symbol else {
            return None;
        };
        Self::resolve_name(module, name)
    }

    fn resolve_name(module: &str, name: &str) -> Option<u32> {
        if module.eq_ignore_ascii_case("dinput.dll") {
            return dinput::Call::resolve(name);
        }
        if module.eq_ignore_ascii_case("advapi32.dll") {
            return Some(
                API_BASE
                    + match name {
                        "RegOpenKeyExA" => 0x278,
                        "RegCreateKeyExA" => 0x27c,
                        "RegCloseKey" => 0x280,
                        "RegQueryValueExA" => 0x284,
                        "RegSetValueExA" => 0x288,
                        "RegSetValueA" => 0x298,
                        _ => return None,
                    },
            );
        }
        if module.eq_ignore_ascii_case("msvcrt.dll") {
            return crt::resolve(name);
        }
        if module.eq_ignore_ascii_case("ole32.dll") {
            return com::Call::resolve(name).map(|offset| API_BASE + offset);
        }
        let offset = if module.eq_ignore_ascii_case("kernel32.dll") {
            Self::kernel32(name)?
        } else if module.eq_ignore_ascii_case("winmm.dll") && name == "timeGetTime" {
            0x244
        } else if module.eq_ignore_ascii_case("d3d8.dll") && name == "Direct3DCreate8" {
            12
        } else if module.eq_ignore_ascii_case("user32.dll") {
            match name {
                "GetDesktopWindow" => 16,
                "FindWindowA" => 0x26c,
                "IsWindow" => 0x270,
                "GetActiveWindow" => 0x430,
                "GetDlgItem" => 0x438,
                "CreateDialogIndirectParamA" => 0x434,
                "wsprintfA" => 0x25c,
                "GetClassInfoA" => 0x260,
                "RegisterClassA" => 0x264,
                "UnregisterClassA" => 0x268,
                "SetWindowsHookExA" => 0x24c,
                "UnhookWindowsHookEx" => 0x250,
                "LoadStringA" => 0xec,
                "LoadIconA" => 0x2c8,
                "LoadAcceleratorsA" => 0x29c,
                "CopyAcceleratorTableA" => 0x2a0,
                "CallWindowProcA" => 0x2a4,
                "SendMessageA" => 0x2cc,
                "PeekMessageA" => 0x43c,
                "GetMessageA" => 0x448,
                "TranslateMessage" => 0x44c,
                "DispatchMessageA" => 0x450,
                "PostMessageA" => 0x464,
                "GetTopWindow" => 0x454,
                "GetWindow" => 0x458,
                "SetWindowTextA" => 0x45c,
                "EnableWindow" => 0x460,
                "EndDialog" => 0x468,
                "SetWindowPos" => 0x46c,
                "DestroyWindow" => 0x470,
                "ShowWindow" => 0x440,
                "UpdateWindow" => 0x444,
                "InvalidateRect" => 0x4b0,
                "SetForegroundWindow" => 0x4b4,
                "CallNextHookEx" => 0x2c4,
                "CreateWindowExA" => 0x2a8,
                "DefWindowProcA" => 0x2ac,
                "GetWindowRect" => 0x2b0,
                "GetClientRect" => 0x2b4,
                "GetParent" => 0x2b8,
                "GetWindowLongA" => 0x2bc,
                "SetWindowLongA" => 0x2c0,
                "RegisterWindowMessageA" => 0x94,
                "RegisterClipboardFormatA" => 0xc8,
                "GetSystemMetrics" => 0x9c,
                "GetSysColor" => 0xac,
                "GetSysColorBrush" => 0xb0,
                "LoadCursorA" => 0xbc,
                "SetCursor" => 0xc0,
                "GetCursor" => 0xc4,
                "GetCursorPos" => 0xcc,
                "SetCursorPos" => 0xd0,
                "GetDC" => 0xa0,
                "ReleaseDC" => 0xa4,
                _ => return None,
            }
        } else if module.eq_ignore_ascii_case("gdi32.dll") {
            match name {
                "GetDeviceCaps" => 0xa8,
                "GetObjectA" => 0xb4,
                "DeleteObject" => 0xb8,
                _ => return None,
            }
        } else {
            return None;
        };
        Some(API_BASE + offset)
    }

    fn kernel32(name: &str) -> Option<u32> {
        let offset = match name {
            "SetLastError" => 0,
            "GetLastError" => 4,
            "ResumeThread" => 0x550,
            "SuspendThread" => 0x554,
            "GetCurrentThread" => 0xd8,
            "GetCurrentThreadId" => 0xdc,
            "SetThreadPriority" => 0x254,
            "GetThreadPriority" => 0x258,
            "ExitProcess" => 8,
            "InterlockedExchange" => 0x200,
            "InterlockedIncrement" => 0x204,
            "InterlockedDecrement" => 0x208,
            "LoadLibraryA" => 0x14,
            "GetModuleHandleA" => 0x18,
            "GetProcAddress" => 0x274,
            "GetModuleFileNameA" => 0xe0,
            "DisableThreadLibraryCalls" => 0xfc,
            "GetSystemDirectoryA" => 0xf8,
            "GetComputerNameA" => 0x20c,
            "CreateMutexA" => 0x210,
            "CreateEventA" => 0x53c,
            "SetEvent" => 0x540,
            "ResetEvent" => 0x544,
            "WaitForSingleObject" => 0x214,
            "ReleaseMutex" => 0x218,
            "CloseHandle" => 0x21c,
            "CreateFileA" => 0x5b4,
            "GetFileSize" => 0x5b8,
            "GetDiskFreeSpaceA" => 0x5bc,
            "SetFilePointer" => 0x5c0,
            "ReadFile" => 0x5c4,
            "QueryPerformanceFrequency" => 0x220,
            "QueryPerformanceCounter" => 0x224,
            "GetCurrentDirectoryA" => 0x228,
            "SetCurrentDirectoryA" => 0x230,
            "FindFirstFileA" => 0x234,
            "FindNextFileA" => 0x238,
            "FindClose" => 0x23c,
            "GetEnvironmentVariableA" => 0x240,
            "GetStartupInfoA" => 0x248,
            "GetCommandLineA" => 0x22c,
            "lstrcpynA" => 0xe4,
            "lstrcpyA" => 0xf0,
            "lstrcatA" => 0xf4,
            "lstrlenA" => 0x28c,
            "GetFileAttributesA" => 0x290,
            "GetShortPathNameA" => 0x294,
            "FindResourceA" => 0xe8,
            "LoadResource" => 0x424,
            "LockResource" => 0x428,
            "SizeofResource" => 0x42c,
            "FreeLibrary" => 0x1c,
            "SetErrorMode" => 0x20,
            "GetErrorMode" => 0x24,
            "LocalAlloc" => 0x28,
            "LocalFree" => 0x2c,
            "LocalReAlloc" => 0xd4,
            "GetVersion" => 0x30,
            "InitializeCriticalSection" => 0x34,
            "EnterCriticalSection" => 0x38,
            "TryEnterCriticalSection" => 0x3c,
            "LeaveCriticalSection" => 0x60,
            "DeleteCriticalSection" => 0x64,
            "TlsAlloc" => 0x68,
            "TlsFree" => 0x6c,
            "TlsGetValue" => 0x70,
            "TlsSetValue" => 0x74,
            "GlobalAlloc" => 0x78,
            "GlobalLock" => 0x7c,
            "GlobalUnlock" => 0x80,
            "GlobalFree" => 0x84,
            "GetACP" => 0x88,
            "GetOEMCP" => 0x8c,
            "GetCPInfo" => 0x90,
            "WideCharToMultiByte" => 0x338,
            "MultiByteToWideChar" => 0x33c,
            "GetProcessVersion" => 0x98,
            _ => return None,
        };
        Some(offset)
    }

    fn arguments(self) -> usize {
        match self {
            Self::Interlocked(call) => call.arguments(),
            Self::Synchronization(call) => call.arguments(),
            Self::Hook(call) => call.arguments(),
            Self::ThreadPriority(call) => call.arguments(),
            Self::Directory(call) => call.arguments(),
            Self::Clock(call) => call.arguments(),
            Self::GetEnvironmentVariable | Self::InvalidateRect => 3,
            Self::WindowsFormat
            | Self::ShowWindow
            | Self::SetWindowText
            | Self::EnableWindow
            | Self::EndDialog
            | Self::CxxThrow => 2,
            Self::CallWindowProc | Self::CreateDialog | Self::PeekMessage => 5,
            Self::SetWindowPos => 7,
            Self::CallNextHook | Self::SendMessage | Self::PostMessage | Self::GetMessage => 4,
            Self::Window(call) => call.arguments(),
            Self::GetLastError
            | Self::GetCommandLine
            | Self::GetCurrentThread
            | Self::GetCurrentThreadId
            | Self::GetErrorMode
            | Self::GetVersion
            | Self::ExceptionProlog
            | Self::Unsupported => 0,
            Self::Graphics(call) => call.arguments(),
            Self::Sound(_) => dsound::Call::arguments(),
            Self::Gdi(call) => call.arguments(),
            Self::Cursor(call) => call.arguments(),
            Self::Class(call) => call.arguments(),
            Self::Desktop(call) => call.arguments(),
            Self::Crt(call) => call.arguments(),
            Self::CodePage(call) => call.arguments(),
            Self::Com(call) => call.arguments(),
            Self::Input(call) => call.arguments(),
            Self::Heap(call) => call.arguments(),
            Self::Tls(call) => call.arguments(),
            Self::Module(call) => call.arguments(),
            Self::Resource(call) => call.arguments(),
            Self::Registry(call) => call.arguments(),
            Self::String(call) => call.arguments(),
            Self::System(call) => call.arguments(),
            _ => 1,
        }
    }

    fn stack_cleanup(self) -> u32 {
        match self {
            Self::Crt(_) | Self::WindowsFormat => 4,
            _ => u32::try_from((self.arguments() + 1) * 4).expect("api frame fits u32"),
        }
    }
}

impl Process32 {
    /// maps the image, a 64 kib stack and a reserved api page within the page cap.
    ///
    /// # errors
    /// rejects unsupported images/imports, allocation limits and reserved-range
    /// collisions. only initial thread fields are supplied; no static tls initialization,
    /// peb initialization or full crt startup is performed.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn load(bytes: &[u8], page_limit: u32) -> Result<Self, LoadError> {
        Self::load_with_options(bytes, page_limit, ProcessOptions::default())
    }

    /// traces entry without resolving every import or initializing missing dlls.
    /// unknown calls stop with their identity before any api work; imported data
    /// accesses fault. this mode does not establish successful process startup.
    ///
    /// # errors
    /// returns the image, memory and reserved-range errors of [`Self::load`].
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn load_diagnostic(bytes: &[u8], page_limit: u32) -> Result<Self, LoadError> {
        Self::load_with_options(
            bytes,
            page_limit,
            ProcessOptions {
                diagnostic_imports: true,
                ..ProcessOptions::default()
            },
        )
    }

    /// loads startup parameters and explicit dll providers, rebasing collisions.
    /// dependency-ordered process attach executes on the first run before the exe.
    ///
    /// # errors
    /// rejects invalid/oversized inputs, cycles, unsupported exports/directories,
    /// and the basic loader's image/memory errors.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn load_with_options(
        bytes: &[u8],
        page_limit: u32,
        options: ProcessOptions<'_>,
    ) -> Result<Self, LoadError> {
        let parameters = parameters::Parameters::prepare(options)?;
        let mut current_directory = directory::Directory::new(
            options.current_directory,
            options.directories,
            options.files,
        )?;
        current_directory.configure_file_contents(options.file_contents_mode)?;
        current_directory.attach_contents(options.file_contents)?;
        let mut diagnostic_imports = diagnostics::Imports::default();
        let reserved = reserved_ranges();
        let all_modules = collect_modules(options.modules, options.deferred_modules);
        let loaded = load_modules(
            bytes,
            page_limit,
            &all_modules,
            options.modules.len(),
            &reserved,
            |module, symbol| {
                Api::resolve(module, symbol).or_else(|| {
                    if options.diagnostic_imports {
                        diagnostic_imports.insert(module, symbol)
                    } else {
                        None
                    }
                })
            },
        )?;
        let (major, minor) = loaded.subsystem_version;
        let mut image = loaded.image;
        let resources =
            resources::Resources::new(image.image_base, bytes, &loaded.providers, &all_modules);
        let modules = modules::Modules::new(
            image.image_base,
            options.image_path,
            loaded.providers,
            options.modules.len(),
            &loaded.initializers,
        );
        image.memory.map_zeroed(
            u64::from(STACK_BASE),
            u64::from(STACK_SIZE),
            Permissions::READ_WRITE,
        )?;
        image
            .memory
            .map_zeroed(u64::from(API_BASE), PAGE_SIZE, Permissions::NONE)?;
        d3d8::Graphics::initialize(&mut image.memory)?;
        diagnostic_imports.map(&mut image.memory)?;
        thread::initialize(&mut image.memory, STACK_BASE, STACK_BASE + STACK_SIZE)?;
        parameters.map(&mut image.memory)?;
        crt::initialize(&mut image.memory, &parameters)?;
        let (startup, entry) =
            startup::Startup::map(&mut image.memory, modules.initializers(), image.entry_point)?;
        let mut cpu = Cpu32::new(entry);
        cpu.set_register(Register32::Esp, STACK_BASE + STACK_SIZE);
        cpu.set_fs_base(thread::BASE);
        cpu.set_x87_control_word(0x027f);
        Ok(Self {
            memory: image.memory,
            cpu,
            exit_code: None,
            error_mode: 0,
            elapsed_nanoseconds: 0,
            command_line: parameters.command_line,
            subsystem_version: (u32::from(major) << 16) | u32::from(minor),
            startup,
            modules,
            resources,
            current_directory,
            environment: environment::Environment::new(options.environment),
            user_atoms: user_atoms::UserAtoms::default(),
            classes: classes::Classes::default(),
            desktop: desktop::Desktop::default(),
            messages: messages::Queue::default(),
            registry: registry::Registry::default(),
            gdi: gdi::Gdi::default(),
            cursors: cursors::Cursors::default(),
            heap: heap::Heap::default(),
            critical_sections: critical_sections::CriticalSections::default(),
            sync_objects: synchronization::SyncObjects::default(),
            threads: threads::Threads::default(),
            hooks: hooks::Hooks::default(),
            tls: tls::Tls::default(),
            graphics: d3d8::Graphics::default(),
            sound_data_mapped: false,
            crt: crt::Crt::default(),
            com: com::Com::default(),
            input: dinput::Input::default(),
            diagnostic_imports,
        })
    }

    /// reads the same guest thread field used by win32 and fs-relative accesses.
    ///
    /// # errors
    /// returns a guest memory fault if the thread field is no longer readable.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn last_error(&self) -> Result<u32, MemoryError> {
        thread::Teb(self.cpu.fs_base()).last_error(&self.memory)
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    #[must_use]
    pub fn crt_application_type(&self) -> i32 {
        self.crt.application_type
    }

    #[must_use]
    pub fn crt_new_mode(&self) -> u32 {
        self.crt.new_mode
    }

    /// takes the latest presented frame; presentation does not accumulate a queue.
    pub fn take_frame(&mut self) -> Option<Frame> {
        self.graphics.take_frame()
    }

    /// returns detached snapshots of owned windows in ascending handle order.
    #[must_use]
    pub fn window_snapshots(&self) -> Vec<WindowSnapshot> {
        self.desktop.snapshots()
    }

    /// replaces the process's immediate keyboard snapshot, initially all released.
    /// indexes are directinput DIK scan codes, not virtual keys or text characters.
    /// reads do not consume it; acquisition and device lifetimes do not clear it.
    /// the host owns key mapping and must clear released keys on host focus loss;
    /// this does not change guest window activation or synthesize messages.
    ///
    /// # errors
    /// rejects an exited process without changing the snapshot.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn set_keyboard_state(&mut self, keys: [bool; 256]) -> Result<(), KeyboardInputError> {
        if self.exit_code.is_some() {
            return Err(KeyboardInputError::Exited);
        }
        self.input.set_keyboard_state(keys);
        Ok(())
    }

    /// adds one host-provided message to this process's bounded thread queue.
    ///
    /// # errors
    /// rejects an exited process, invalid window or message id, or full queue.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn post_message(&mut self, message: PostedMessage) -> Result<(), PostMessageError> {
        if self.exit_code.is_some() {
            return Err(PostMessageError::Exited);
        }
        if message.message > u16::MAX.into() || (message.message == 0x12 && message.hwnd != 0) {
            return Err(PostMessageError::InvalidMessage);
        }
        if message.hwnd != 0 && self.desktop.window(message.hwnd).is_none() {
            return Err(PostMessageError::InvalidWindow);
        }
        self.messages.post(message)
    }

    /// each guest execution step and dispatched api call costs one budget unit.
    /// resumed threads share a 4096-operation quantum independent of host budgets.
    /// highest relative priority runs first; equal priorities rotate in creation order.
    /// dll initialization pins its owning thread until the notification finishes.
    /// the public cpu is the selected thread; scheduling validates its fs identity.
    /// a dispatched callback may still be in progress when the budget ends.
    /// an event wait is charged when parked; its released continuation costs no unit.
    /// finite event waits expire at a positive run boundary using the supplied elapsed time.
    /// when no thread is ready, returns the synchronization wait without more work.
    /// the host may advance elapsed time and run again; execution never advances the clock.
    /// a pending file-content request pauses every thread before scheduling on positive
    /// budgets; supplying its snapshot permits the original file open to be retried.
    /// repeated strings use one step per element, or one for a zero-count operation.
    /// faults consume no unit for the faulting operation. exit is terminal and
    /// later calls return the same code without executing more guest work.
    pub fn run(&mut self, budget: u64) -> ProcessResult {
        let mut result = ProcessResult {
            reason: ProcessStop::Stopped(StopReason::InstructionLimit),
            instructions: 0,
            api_calls: 0,
        };
        if let Some(code) = self.exit_code {
            result.reason = ProcessStop::Exited(code);
            return result;
        }
        if let Some(stop) = self.startup.failed() {
            result.reason = stop;
            return result;
        }
        if budget != 0 && self.pending_file_contents().is_some() {
            result.reason = ProcessStop::FileContentsRequired;
            return result;
        }
        let mut remaining = budget;
        while remaining != 0 {
            let slice = match self.schedule_slice(remaining) {
                Ok(Some(slice)) => slice,
                Ok(None) => {
                    result.reason = ProcessStop::WaitingForSynchronization;
                    return result;
                }
                Err(error) => {
                    result.reason = error.stop(self.cpu.eip);
                    return result;
                }
            };
            match self.threads.finish_wait(&mut self.cpu, &self.memory) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    result.reason = error.stop(self.cpu.eip);
                    return result;
                }
            }
            let step = self.cpu.run_until(&mut self.memory, slice, |address| {
                Api::at(address).is_some()
                    || self.diagnostic_imports.contains(address)
                    || self.startup.contains(address)
                    || address == callbacks::RETURN
                    || address == eh::RETURN
                    || address == threads::ENTER
                    || address == threads::RETURN
            });
            result.instructions += step.instructions;
            remaining -= step.instructions;
            self.threads.account(step.instructions);
            if step.reason == StopReason::InstructionLimit {
                continue;
            }
            if step.reason != StopReason::Intercepted {
                result.reason = ProcessStop::Stopped(step.reason);
                return result;
            }
            match self.resume_continuation() {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    result.reason = error.stop(self.cpu.eip);
                    return result;
                }
            }
            if let Some(stop) = self
                .startup
                .stop(self.cpu.eip)
                .or_else(|| self.diagnostic_imports.stop(self.cpu.eip))
            {
                result.reason = stop;
                return result;
            }
            let Some(api) = Api::at(self.cpu.eip) else {
                unreachable!()
            };
            if matches!(api, Api::Unsupported) {
                result.reason = ProcessStop::UnsupportedApi {
                    address: self.cpu.eip,
                };
                return result;
            }
            if let Err(error) = self.dispatch(api) {
                result.reason = error.stop(self.cpu.eip);
                return result;
            }
            result.api_calls += 1;
            remaining -= 1;
            self.threads.account(1);
            if let Some(code) = self.exit_code {
                result.reason = ProcessStop::Exited(code);
                return result;
            }
        }
        result
    }

    fn resume_continuation(&mut self) -> Result<bool, DispatchError> {
        if self.startup.complete_at(self.cpu.eip) {
            return Ok(true);
        }
        match self.cpu.eip {
            threads::ENTER | threads::RETURN => self.advance_thread_entry()?,
            callbacks::RETURN => self.finish_callback()?,
            eh::RETURN => self.finish_exception_call()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn dispatch(&mut self, api: Api) -> Result<(), DispatchError> {
        if (self.threads.scheduled_child() && api.requires_primary())
            || (matches!(api, Api::Input(_)) && self.cpu.fs_base() != thread::BASE)
        {
            return Err(DispatchError::Unsupported);
        }
        let stack = self.cpu.register(Register32::Esp);
        let words = api.arguments() + 1;
        let mut frame = [0; 13];
        guest::read_words(&self.memory, stack, &mut frame[..words])?;
        if matches!(
            api,
            Api::SuspendThread
                | Api::ResumeThread
                | Api::CloseHandle
                | Api::Directory(
                    directory::Call::OpenFile
                        | directory::Call::FileSize
                        | directory::Call::DiskGeometry
                        | directory::Call::SeekFile
                        | directory::Call::ReadFile
                )
                | Api::Input(_)
                | Api::Hook(_)
                | Api::Crt(crt::Call::Sort)
        ) {
            stack
                .checked_add(api.stack_cleanup())
                .ok_or(MemoryError::AddressOverflow)?;
        }
        if matches!(api, Api::CallWindowProc) {
            return self.threads.state_mut(self.cpu.fs_base())?.callbacks.start(
                &mut self.cpu,
                &mut self.memory,
                &frame[1..words],
            );
        }
        if matches!(api, Api::ExceptionProlog) {
            return crt::enter_exception_frame(&mut self.cpu, &mut self.memory, frame[0]);
        }
        if matches!(api, Api::CxxThrow) {
            return self.throw_exception(&frame[1..3]);
        }
        if matches!(api, Api::ExitProcess) {
            self.exit_code = Some(frame[1]);
            return Ok(());
        }
        let suspended = match api {
            Api::Crt(crt::Call::Sort) => self.sort(&frame[1..words])?,
            Api::Synchronization(synchronization::Call::Wait) => {
                self.wait_event(&frame[1..words])?
            }
            Api::SendMessage => self.send_message(&frame[1..words])?,
            Api::UpdateWindow => self.update_window(frame[1])?,
            Api::DestroyWindow => self.destroy_dialog(frame[1])?,
            Api::DispatchMessage => self.dispatch_message(frame[1])?,
            Api::CreateDialog => self.create_dialog(&frame[1..words])?,
            Api::CallNextHook => self.call_next_hook(&frame[1..words])?,
            Api::Window(creation::Call::Create) => self.create_window(&frame[1..words])?,
            Api::Module(modules::Call::Load) => self.load_module(&frame[1..words])?,
            Api::Sound(dsound::Call::EnumerateA) => self.enumerate_sound(&frame[1..words])?,
            _ => {
                self.invoke(api, &frame[1..words], stack)?;
                false
            }
        };
        if suspended {
            return Ok(());
        }
        // an api output may alias the saved return address; heap free protects this frame.
        guest::read_words(&self.memory, stack, &mut frame[..1])?;
        self.cpu
            .set_register(Register32::Esp, stack.wrapping_add(api.stack_cleanup()));
        self.cpu.eip = frame[0];
        Ok(())
    }

    fn load_module(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        let initialized = self.startup.is_complete();
        match self.modules.load(
            args[0],
            initialized,
            thread::Teb(self.cpu.fs_base()),
            &mut self.memory,
        )? {
            modules::Load::Complete(value) => {
                self.cpu.set_register(Register32::Eax, value);
                Ok(false)
            }
            modules::Load::Initialize(pending) => {
                let stack = self.cpu.register(Register32::Esp);
                self.threads
                    .state_mut(self.cpu.fs_base())?
                    .callbacks
                    .enter(
                        &mut self.cpu,
                        &mut self.memory,
                        callbacks::Frame {
                            stack,
                            caller: stack,
                            cleanup: 8,
                            creation: None,
                            cbt_hook: None,
                            module: Some(pending),
                            dialog: None,
                            destroy: None,
                            paint: false,
                            sound_enumeration: false,
                        },
                        pending.entry,
                        &[pending.handle, 1, 0],
                    )?;
                self.modules.start(pending, self.cpu.fs_base());
                Ok(true)
            }
        }
    }

    fn set_error_mode(&mut self, argument: u32) -> Result<(), DispatchError> {
        if argument & !0x8007 != 0 {
            return Err(DispatchError::Unsupported);
        }
        self.cpu.set_register(Register32::Eax, self.error_mode);
        // x86 permits ignoring sem_noalignmentfaultexcept.
        self.error_mode = argument & 0x8003;
        Ok(())
    }

    fn process_version(&mut self, module: u32) -> Result<(), DispatchError> {
        if module != 0 {
            return Err(DispatchError::Unsupported);
        }
        self.cpu
            .set_register(Register32::Eax, self.subsystem_version);
        Ok(())
    }

    fn peek_message(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if (!matches!(args[1], 0 | u32::MAX) && self.desktop.window(args[1]).is_none())
            || args[2] > u16::MAX.into()
            || args[3] > u16::MAX.into()
            || args[4] & !3 != 0
        {
            return Err(DispatchError::Unsupported);
        }
        self.receive_message(&args[..4], args[4] & 1 != 0, false)
    }

    fn get_message(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if (!matches!(args[1], 0 | u32::MAX) && self.desktop.window(args[1]).is_none())
            || args[2] > u16::MAX.into()
            || args[3] > u16::MAX.into()
        {
            return Err(DispatchError::Unsupported);
        }
        self.receive_message(args, true, true)
    }

    fn translate_message(&mut self, address: u32) -> Result<(), DispatchError> {
        let mut words = [0; 8];
        guest::read_words(&self.memory, address, &mut words)?;
        let translated = matches!(words[1], 0x100 | 0x101 | 0x104 | 0x105);
        if matches!(words[1], 0x100 | 0x104) && words[2] == 13 {
            self.post_message(PostedMessage {
                hwnd: words[0],
                message: if words[1] == 0x100 { 0x102 } else { 0x106 },
                wparam: 13,
                lparam: words[3],
                time: words[4],
                point: [words[5].cast_signed(), words[6].cast_signed()],
            })
            .map_err(|_| DispatchError::Unsupported)?;
        }
        self.cpu
            .set_register(Register32::Eax, u32::from(translated));
        Ok(())
    }

    fn post_guest_message(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if matches!(args[0], 0xffff | u32::MAX) || args[1] > u16::MAX.into() {
            return Err(DispatchError::Unsupported);
        }
        let millis = (self.elapsed_nanoseconds / 1_000_000) & i64::from(u32::MAX);
        let posted = PostedMessage {
            hwnd: args[0],
            message: args[1],
            wparam: args[2],
            lparam: args[3],
            time: u32::try_from(millis).expect("masked millisecond counter"),
            point: self.cursors.position(),
        };
        match self.post_message(posted) {
            Ok(()) => self.cpu.set_register(Register32::Eax, 1),
            Err(PostMessageError::InvalidWindow) => {
                thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
                self.cpu.set_register(Register32::Eax, 0);
            }
            Err(PostMessageError::Full) => {
                thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1816)?;
                self.cpu.set_register(Register32::Eax, 0);
            }
            Err(PostMessageError::InvalidMessage | PostMessageError::Exited) => {
                return Err(DispatchError::Unsupported);
            }
        }
        Ok(())
    }

    fn message_api(&mut self, api: Api, args: &[u32]) -> Result<(), DispatchError> {
        match api {
            Api::PeekMessage => self.peek_message(args),
            Api::GetMessage => self.get_message(args),
            Api::TranslateMessage => self.translate_message(args[0]),
            _ => unreachable!(),
        }
    }

    fn receive_message(
        &mut self,
        args: &[u32],
        remove: bool,
        wait: bool,
    ) -> Result<(), DispatchError> {
        guest::check(&self.memory, args[0], 32, Access::Write)?;
        let Some((index, message)) = self.messages.find(args[1], args[2], args[3], &self.desktop)
        else {
            if wait {
                return Err(DispatchError::WaitingForMessage);
            }
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        };
        self.memory.write(u64::from(args[0]), &message.bytes())?;
        if remove {
            self.messages.remove(index);
        }
        self.cpu
            .set_register(Register32::Eax, u32::from(!wait || message.message != 0x12));
        Ok(())
    }

    fn show_window(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        let was_visible = match args[1] {
            0 => self.desktop.hide_window(args[0]),
            1 | 5 => self.desktop.show_activated(args[0]),
            _ => return Err(DispatchError::Unsupported),
        };
        let Some(was_visible) = was_visible else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        };
        self.cpu.set_register(Register32::Eax, was_visible);
        Ok(())
    }

    fn update_window(&mut self, handle: u32) -> Result<bool, DispatchError> {
        if handle == desktop::DESKTOP {
            return Err(DispatchError::Unsupported);
        }
        let Some(window) = self.desktop.window(handle) else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(false);
        };
        if window.class != 0x8002 || window.dialog_units.is_none() || window.procedure == 0 {
            return Err(DispatchError::Unsupported);
        }
        if !window.needs_paint {
            self.cpu.set_register(Register32::Eax, 1);
            return Ok(false);
        }
        let procedure = window.procedure;
        let stack = self.cpu.register(Register32::Esp);
        self.threads
            .state_mut(self.cpu.fs_base())?
            .callbacks
            .enter(
                &mut self.cpu,
                &mut self.memory,
                callbacks::Frame {
                    stack,
                    caller: stack,
                    cleanup: 8,
                    creation: None,
                    cbt_hook: None,
                    module: None,
                    dialog: None,
                    destroy: None,
                    paint: true,
                    sound_enumeration: false,
                },
                procedure,
                &[handle, 0x0f, 0, 0],
            )?;
        self.desktop
            .window_mut(handle)
            .expect("validated window")
            .needs_paint = false;
        Ok(true)
    }

    fn invalidate_rect(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if args[1] != 0 || args[2] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if self.desktop.invalidate_full(args[0]) {
            self.cpu.set_register(Register32::Eax, 1);
        } else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
        }
        Ok(())
    }

    fn com_api(&mut self, call: com::Call, args: &[u32]) -> Result<(), DispatchError> {
        if let Some(value) =
            self.com
                .dispatch(call, args, &mut self.memory, &self.current_directory)?
        {
            self.cpu.set_register(Register32::Eax, value);
        }
        Ok(())
    }

    #[expect(clippy::too_many_lines, reason = "flat api routing table")]
    fn invoke(&mut self, api: Api, args: &[u32], stack: u32) -> Result<(), DispatchError> {
        let argument = args.first().copied().unwrap_or(0);
        match api {
            Api::Interlocked(call) => self.interlocked(call, args)?,
            Api::Clock(call) => self.query_clock(call, argument)?,
            Api::Directory(call) => self.directory(call, args)?,
            Api::CloseHandle => self.close_handle(argument)?,
            Api::Registry(call) => self.registry(call, args)?,
            Api::GetCommandLine => self.cpu.set_register(Register32::Eax, self.command_line),
            Api::GetEnvironmentVariable => self.environment_query(args)?,
            Api::GetStartupInfo => parameters::startup_info(&mut self.memory, argument)?,
            Api::WindowsFormat => self.windows_format(args, stack)?,
            Api::PeekMessage | Api::GetMessage | Api::TranslateMessage => {
                self.message_api(api, args)?;
            }
            Api::PostMessage => self.post_guest_message(args)?,
            Api::ShowWindow => self.show_window(args)?,
            Api::SetForegroundWindow => self.cpu.set_register(
                Register32::Eax,
                u32::from(self.desktop.activate_foreground(argument)),
            ),
            Api::InvalidateRect => self.invalidate_rect(args)?,
            Api::SetWindowText => self.set_window_text(args)?,
            Api::EnableWindow => self.enable_dialog_control(args)?,
            Api::EndDialog => self.end_dialog(args)?,
            Api::SetWindowPos => self.set_dialog_window_pos(args)?,
            Api::Class(call) => self.window_class(call, args)?,
            Api::Window(call) => self.window_api(call, args)?,
            Api::Synchronization(call) => {
                self.synchronization(call, args)?;
            }
            Api::SetLastError => {
                thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, argument)?;
            }
            Api::GetLastError => self.cpu.set_register(Register32::Eax, self.last_error()?),
            Api::ResumeThread => self.resume_thread(argument)?,
            Api::SuspendThread => {
                let value = self.threads.suspend(
                    argument,
                    thread::Teb(self.cpu.fs_base()),
                    self.modules.loader_owner(),
                    &self.sync_objects,
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::GetCurrentThread => self.cpu.set_register(Register32::Eax, u32::MAX - 1),
            Api::GetCurrentThreadId => self.cpu.set_register(
                Register32::Eax,
                thread::Teb(self.cpu.fs_base()).current_id(&self.memory)?,
            ),
            Api::Desktop(call) => self.window_query(call, args)?,
            Api::RegisterUserAtom => self.register_user_atom(argument)?,
            Api::System(call) => {
                let value =
                    call.dispatch(args, thread::Teb(self.cpu.fs_base()), &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::SetErrorMode => self.set_error_mode(argument)?,
            Api::GetErrorMode => self.cpu.set_register(Register32::Eax, self.error_mode),
            Api::GetVersion => self.cpu.set_register(Register32::Eax, GUEST_VERSION),
            Api::GetProcessVersion => self.process_version(argument)?,
            Api::CodePage(call) => {
                let value =
                    call.dispatch(args, thread::Teb(self.cpu.fs_base()), &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Com(call) => self.com_api(call, args)?,
            Api::Input(call) => {
                let value = self
                    .input
                    .dispatch(call, args, &mut self.memory, &self.desktop)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::String(call) => {
                let value =
                    call.dispatch(&mut self.memory, thread::Teb(self.cpu.fs_base()), args)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Tls(call) => {
                let value = self.tls.dispatch(
                    call,
                    args,
                    thread::Teb(self.cpu.fs_base()),
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::CriticalSection(call) => self.critical_section(call, argument)?,
            Api::Heap(call) => {
                let value = self.heap.dispatch(
                    call,
                    args,
                    stack,
                    thread::Teb(self.cpu.fs_base()),
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Module(call) => {
                let attached = self.startup.is_complete();
                let value = self.modules.dispatch(
                    call,
                    args,
                    attached,
                    thread::Teb(self.cpu.fs_base()),
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Resource(call) => self.resource_api(call, args)?,
            Api::Graphics(call) => {
                let value = self
                    .graphics
                    .dispatch(call, args, &mut self.memory, &self.desktop)?;
                if !matches!(call, d3d8::Call::TexturePreLoad) || value != 0 {
                    self.cpu.set_register(Register32::Eax, value);
                }
            }
            Api::Gdi(call) => self.cpu.set_register(
                Register32::Eax,
                self.gdi.dispatch(call, args, &mut self.memory)?,
            ),
            Api::ThreadPriority(call) => {
                self.cpu.set_register(
                    Register32::Eax,
                    self.threads.priority(
                        call,
                        args,
                        thread::Teb(self.cpu.fs_base()),
                        &self.sync_objects,
                        &mut self.memory,
                    )?,
                );
            }
            Api::Hook(call) => self.cpu.set_register(
                Register32::Eax,
                self.hooks.dispatch(
                    call,
                    args,
                    &self.modules,
                    self.threads
                        .id(thread::Teb(self.cpu.fs_base()))
                        .ok_or(DispatchError::Unsupported)?,
                    thread::Teb(self.cpu.fs_base()),
                    &mut self.memory,
                )?,
            ),
            Api::Cursor(call) => self.cpu.set_register(
                Register32::Eax,
                self.cursors.dispatch(
                    call,
                    args,
                    thread::Teb(self.cpu.fs_base()),
                    &mut self.memory,
                )?,
            ),
            Api::Crt(call) => self.crt_call(call, args)?,
            Api::Sound(_)
            | Api::CreateDialog
            | Api::DestroyWindow
            | Api::UpdateWindow
            | Api::CallWindowProc
            | Api::SendMessage
            | Api::DispatchMessage
            | Api::CallNextHook
            | Api::ExceptionProlog
            | Api::CxxThrow
            | Api::ExitProcess
            | Api::Unsupported => unreachable!(),
        }
        Ok(())
    }
}
