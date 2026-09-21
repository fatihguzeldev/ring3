use super::loader::load_modules;
use super::{
    Cpu32, GuestMemory, LoadError, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason,
};
use crate::PeImportSymbol;

mod atomics;
mod code_pages;
mod critical_sections;
mod crt;
mod cursors;
mod d3d8;
mod diagnostics;
mod gdi;
mod guest;
mod heap;
mod modules;
mod parameters;
mod resources;
mod startup;
mod strings;
mod system;
mod thread;
mod tls;
mod user_atoms;

pub use d3d8::Frame;
pub use parameters::ProcessOptions;

const API_BASE: u32 = 0x7000_0000;
const STACK_BASE: u32 = 0x1000_0000;
const STACK_SIZE: u32 = 64 * 1024;
// immutable nt 5.1/build 2600 guest identity, independent of the host and executable.
const GUEST_VERSION: u32 = (2600 << 16) | (1 << 8) | 5;

/// a single guest thread with a minimal win32 import boundary, not a full process.
pub struct Process32 {
    pub memory: GuestMemory,
    pub cpu: Cpu32,
    exit_code: Option<u32>,
    error_mode: u32,
    subsystem_version: u32,
    startup: startup::Startup,
    modules: modules::Modules,
    resources: resources::Resources,
    user_atoms: user_atoms::UserAtoms,
    cursors: cursors::Cursors,
    heap: heap::Heap,
    critical_sections: critical_sections::CriticalSections,
    tls: tls::Tls,
    graphics: d3d8::Graphics,
    gdi: gdi::Gdi,
    crt: crt::Crt,
    diagnostic_imports: diagnostics::Imports,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessStop {
    Exited(u32),
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
    ExitProcess,
    Interlocked(atomics::Call),
    GetDesktopWindow,
    RegisterUserAtom,
    System(system::Call),
    SetErrorMode,
    GetErrorMode,
    GetVersion,
    GetProcessVersion,
    String(strings::Call),
    ExceptionProlog,
    Graphics(d3d8::Call),
    Gdi(gdi::Call),
    Cursor(cursors::Call),
    Crt(crt::Call),
    CodePage(code_pages::Call),
    Module(modules::Call),
    Resource(resources::Call),
    Heap(heap::Call),
    CriticalSection(critical_sections::Call),
    Tls(tls::Call),
    Unsupported,
}

enum DispatchError {
    Memory(MemoryError),
    Unsupported,
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
            0xd8 => Some(Self::GetCurrentThread),
            0xdc => Some(Self::GetCurrentThreadId),
            8 => Some(Self::ExitProcess),
            0x200 => Some(Self::Interlocked(atomics::Call::Exchange)),
            0x204 => Some(Self::Interlocked(atomics::Call::Increment)),
            0x208 => Some(Self::Interlocked(atomics::Call::Decrement)),
            16 => Some(Self::GetDesktopWindow),
            0x20 => Some(Self::SetErrorMode),
            0x24 => Some(Self::GetErrorMode),
            0x30 => Some(Self::GetVersion),
            0x94 | 0xc8 => Some(Self::RegisterUserAtom),
            0x98 => Some(Self::GetProcessVersion),
            0x118 => Some(Self::ExceptionProlog),
            0xffc => Some(Self::Unsupported),
            offset => d3d8::Call::at(offset)
                .map(Self::Graphics)
                .or_else(|| system::Call::at(offset).map(Self::System))
                .or_else(|| gdi::Call::at(offset).map(Self::Gdi))
                .or_else(|| cursors::Call::at(offset).map(Self::Cursor))
                .or_else(|| crt::Call::at(offset).map(Self::Crt))
                .or_else(|| modules::Call::at(offset).map(Self::Module))
                .or_else(|| resources::Call::at(offset).map(Self::Resource))
                .or_else(|| strings::Call::at(offset).map(Self::String))
                .or_else(|| heap::Call::at(offset).map(Self::Heap))
                .or_else(|| critical_sections::Call::at(offset).map(Self::CriticalSection))
                .or_else(|| tls::Call::at(offset).map(Self::Tls))
                .or_else(|| code_pages::Call::at(offset).map(Self::CodePage)),
        }
    }

    fn resolve(module: &str, symbol: PeImportSymbol<'_>) -> Option<u32> {
        let PeImportSymbol::ByName { name, .. } = symbol else {
            return None;
        };
        if module.eq_ignore_ascii_case("msvcrt.dll") {
            return crt::resolve(name);
        }
        let offset = if module.eq_ignore_ascii_case("kernel32.dll") {
            match name {
                "SetLastError" => 0,
                "GetLastError" => 4,
                "GetCurrentThread" => 0xd8,
                "GetCurrentThreadId" => 0xdc,
                "ExitProcess" => 8,
                "InterlockedExchange" => 0x200,
                "InterlockedIncrement" => 0x204,
                "InterlockedDecrement" => 0x208,
                "LoadLibraryA" => 0x14,
                "GetModuleHandleA" => 0x18,
                "GetModuleFileNameA" => 0xe0,
                "DisableThreadLibraryCalls" => 0xfc,
                "GetSystemDirectoryA" => 0xf8,
                "GetComputerNameA" => 0x20c,
                "lstrcpynA" => 0xe4,
                "lstrcpyA" => 0xf0,
                "lstrcatA" => 0xf4,
                "FindResourceA" => 0xe8,
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
                "GetProcessVersion" => 0x98,
                _ => return None,
            }
        } else if module.eq_ignore_ascii_case("d3d8.dll") && name == "Direct3DCreate8" {
            12
        } else if module.eq_ignore_ascii_case("user32.dll") {
            match name {
                "GetDesktopWindow" => 16,
                "LoadStringA" => 0xec,
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

    fn arguments(self) -> usize {
        match self {
            Self::Interlocked(call) => call.arguments(),
            Self::GetLastError
            | Self::GetCurrentThread
            | Self::GetCurrentThreadId
            | Self::GetDesktopWindow
            | Self::GetErrorMode
            | Self::GetVersion
            | Self::ExceptionProlog
            | Self::Unsupported => 0,
            Self::Graphics(call) => call.arguments(),
            Self::Gdi(call) => call.arguments(),
            Self::Cursor(call) => call.arguments(),
            Self::Crt(call) => call.arguments(),
            Self::CodePage(call) => call.arguments(),
            Self::Heap(call) => call.arguments(),
            Self::Tls(call) => call.arguments(),
            Self::Module(call) => call.arguments(),
            Self::Resource(call) => call.arguments(),
            Self::String(call) => call.arguments(),
            Self::System(call) => call.arguments(),
            _ => 1,
        }
    }

    fn stack_cleanup(self) -> u32 {
        match self {
            Self::Crt(_) => 4,
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
        let mut diagnostic_imports = diagnostics::Imports::default();
        let reserved = [
            u64::from(STACK_BASE)..u64::from(STACK_BASE + STACK_SIZE),
            heap::START..heap::END,
            u64::from(API_BASE)..u64::from(startup::BASE) + PAGE_SIZE,
            u64::from(diagnostics::BASE)
                ..u64::from(diagnostics::BASE) + u64::from(diagnostics::MAX_IMPORTS) * 4,
            u64::from(thread::BASE)..u64::from(thread::BASE) + PAGE_SIZE,
        ];
        let loaded = load_modules(
            bytes,
            page_limit,
            options.modules,
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
            resources::Resources::new(image.image_base, bytes, &loaded.providers, options.modules);
        let modules = modules::Modules::new(image.image_base, options.image_path, loaded.providers);
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
            startup::Startup::map(&mut image.memory, loaded.initializers, image.entry_point)?;
        let mut cpu = Cpu32::new(entry);
        cpu.set_register(Register32::Esp, STACK_BASE + STACK_SIZE);
        cpu.set_fs_base(thread::BASE);
        cpu.set_x87_control_word(0x027f);
        Ok(Self {
            memory: image.memory,
            cpu,
            exit_code: None,
            error_mode: 0,
            subsystem_version: (u32::from(major) << 16) | u32::from(minor),
            startup,
            modules,
            resources,
            user_atoms: user_atoms::UserAtoms::default(),
            gdi: gdi::Gdi::default(),
            cursors: cursors::Cursors::default(),
            heap: heap::Heap::default(),
            critical_sections: critical_sections::CriticalSections::default(),
            tls: tls::Tls::default(),
            graphics: d3d8::Graphics::default(),
            crt: crt::Crt::default(),
            diagnostic_imports,
        })
    }

    /// reads the same guest thread field used by win32 and fs-relative accesses.
    ///
    /// # errors
    /// returns a guest memory fault if the thread field is no longer readable.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn last_error(&self) -> Result<u32, MemoryError> {
        thread::last_error(&self.memory)
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

    /// each guest execution step and completed api call costs one budget unit.
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
        let mut remaining = budget;
        while remaining != 0 {
            let step = self.cpu.run_until(&mut self.memory, remaining, |address| {
                Api::at(address).is_some()
                    || self.diagnostic_imports.contains(address)
                    || self.startup.contains(address)
            });
            result.instructions += step.instructions;
            remaining -= step.instructions;
            if step.reason != StopReason::Intercepted {
                result.reason = ProcessStop::Stopped(step.reason);
                return result;
            }
            if self.startup.complete_at(self.cpu.eip) {
                continue;
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
                result.reason = match error {
                    DispatchError::Memory(error) => {
                        ProcessStop::Stopped(StopReason::MemoryFault(error))
                    }
                    DispatchError::Unsupported => ProcessStop::UnsupportedApi {
                        address: self.cpu.eip,
                    },
                };
                return result;
            }
            result.api_calls += 1;
            remaining -= 1;
            if let Some(code) = self.exit_code {
                result.reason = ProcessStop::Exited(code);
                return result;
            }
        }
        result
    }

    fn dispatch(&mut self, api: Api) -> Result<(), DispatchError> {
        let stack = self.cpu.register(Register32::Esp);
        let words = api.arguments() + 1;
        let mut frame = [0; 8];
        guest::read_words(&self.memory, stack, &mut frame[..words])?;
        if matches!(api, Api::ExceptionProlog) {
            return crt::enter_exception_frame(&mut self.cpu, &mut self.memory, frame[0]);
        }
        if matches!(api, Api::ExitProcess) {
            self.exit_code = Some(frame[1]);
            return Ok(());
        }
        self.invoke(api, &frame[1..words], stack)?;
        // an api output may alias the saved return address; heap free protects this frame.
        guest::read_words(&self.memory, stack, &mut frame[..1])?;
        self.cpu
            .set_register(Register32::Esp, stack.wrapping_add(api.stack_cleanup()));
        self.cpu.eip = frame[0];
        Ok(())
    }

    fn invoke(&mut self, api: Api, arguments: &[u32], stack: u32) -> Result<(), DispatchError> {
        let argument = arguments.first().copied().unwrap_or(0);
        match api {
            Api::Interlocked(call) => self.interlocked(call, arguments)?,
            Api::SetLastError => thread::set_last_error(&mut self.memory, argument)?,
            Api::GetLastError => self.cpu.set_register(Register32::Eax, self.last_error()?),
            Api::GetCurrentThread => self.cpu.set_register(Register32::Eax, u32::MAX - 1),
            Api::GetCurrentThreadId => self
                .cpu
                .set_register(Register32::Eax, thread::current_id(&self.memory)?),
            Api::GetDesktopWindow => self.cpu.set_register(Register32::Eax, d3d8::DESKTOP),
            Api::RegisterUserAtom => {
                let value = self.user_atoms.register(argument, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::System(call) => self
                .cpu
                .set_register(Register32::Eax, call.dispatch(arguments, &mut self.memory)?),
            Api::SetErrorMode => {
                if argument & !0x8007 != 0 {
                    return Err(DispatchError::Unsupported);
                }
                self.cpu.set_register(Register32::Eax, self.error_mode);
                // x86 permits ignoring sem_noalignmentfaultexcept.
                self.error_mode = argument & 0x8003;
            }
            Api::GetErrorMode => self.cpu.set_register(Register32::Eax, self.error_mode),
            Api::GetVersion => self.cpu.set_register(Register32::Eax, GUEST_VERSION),
            Api::GetProcessVersion => {
                if argument != 0 {
                    return Err(DispatchError::Unsupported);
                }
                self.cpu
                    .set_register(Register32::Eax, self.subsystem_version);
            }
            Api::CodePage(call) => {
                let value = call.dispatch(arguments, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::String(call) => {
                let value = call.dispatch(&mut self.memory, arguments)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Tls(call) => {
                let value = self.tls.dispatch(call, arguments, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::CriticalSection(call) => self.critical_section(call, argument)?,
            Api::Heap(call) => {
                let value = self
                    .heap
                    .dispatch(call, arguments, stack, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Module(call) => {
                let value = self.modules.dispatch(
                    call,
                    arguments,
                    self.startup.is_complete(),
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Resource(call) => {
                let value =
                    self.resources
                        .dispatch(call, arguments, &self.modules, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Graphics(call) => self.cpu.set_register(
                Register32::Eax,
                self.graphics.dispatch(call, arguments, &mut self.memory)?,
            ),
            Api::Gdi(call) => self.cpu.set_register(
                Register32::Eax,
                self.gdi.dispatch(call, arguments, &mut self.memory)?,
            ),
            Api::Cursor(call) => self.cpu.set_register(
                Register32::Eax,
                self.cursors.dispatch(call, arguments, &mut self.memory)?,
            ),
            Api::Crt(call) => {
                if let Some(value) = self.crt.dispatch(
                    call,
                    arguments,
                    &mut self.cpu,
                    &mut self.memory,
                    &mut self.heap,
                )? {
                    self.cpu.set_register(Register32::Eax, value);
                }
            }
            Api::ExceptionProlog | Api::ExitProcess | Api::Unsupported => unreachable!(),
        }
        Ok(())
    }
}
