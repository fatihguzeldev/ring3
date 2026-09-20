use super::loader::load_modules;
use super::{
    Cpu32, GuestMemory, LoadError, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason,
};
use crate::PeImportSymbol;

mod code_pages;
mod critical_sections;
mod crt;
mod d3d8;
mod diagnostics;
mod guest;
mod heap;
mod modules;
mod parameters;
mod startup;
mod thread;
mod tls;

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
    startup: startup::Startup,
    modules: modules::Modules,
    heap: heap::Heap,
    critical_sections: critical_sections::CriticalSections,
    tls: tls::Tls,
    graphics: d3d8::Graphics,
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
    pub instructions: u64,
    pub api_calls: u64,
}

#[derive(Clone, Copy)]
enum Api {
    SetLastError,
    GetLastError,
    ExitProcess,
    GetDesktopWindow,
    SetErrorMode,
    GetErrorMode,
    GetVersion,
    ExceptionProlog,
    Graphics(d3d8::Call),
    Crt(crt::Call),
    CodePage(code_pages::Call),
    Module(modules::Call),
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
            8 => Some(Self::ExitProcess),
            16 => Some(Self::GetDesktopWindow),
            0x20 => Some(Self::SetErrorMode),
            0x24 => Some(Self::GetErrorMode),
            0x30 => Some(Self::GetVersion),
            0x118 => Some(Self::ExceptionProlog),
            0xffc => Some(Self::Unsupported),
            offset => d3d8::Call::at(offset)
                .map(Self::Graphics)
                .or_else(|| crt::Call::at(offset).map(Self::Crt))
                .or_else(|| modules::Call::at(offset).map(Self::Module))
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
                "ExitProcess" => 8,
                "LoadLibraryA" => 0x14,
                "GetModuleHandleA" => 0x18,
                "FreeLibrary" => 0x1c,
                "SetErrorMode" => 0x20,
                "GetErrorMode" => 0x24,
                "LocalAlloc" => 0x28,
                "LocalFree" => 0x2c,
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
                _ => return None,
            }
        } else if module.eq_ignore_ascii_case("d3d8.dll") && name == "Direct3DCreate8" {
            12
        } else if module.eq_ignore_ascii_case("user32.dll") && name == "GetDesktopWindow" {
            16
        } else {
            return None;
        };
        Some(API_BASE + offset)
    }

    fn arguments(self) -> usize {
        match self {
            Self::GetLastError
            | Self::GetDesktopWindow
            | Self::GetErrorMode
            | Self::GetVersion
            | Self::ExceptionProlog
            | Self::Unsupported => 0,
            Self::Graphics(call) => call.arguments(),
            Self::Crt(call) => call.arguments(),
            Self::CodePage(call) => call.arguments(),
            Self::Heap(call) => call.arguments(),
            Self::Tls(call) => call.arguments(),
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

    /// loads startup parameters and explicit preferred-base dll providers.
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
        let loaded = load_modules(bytes, page_limit, options.modules, |module, symbol| {
            Api::resolve(module, symbol).or_else(|| {
                if options.diagnostic_imports {
                    diagnostic_imports.insert(module, symbol)
                } else {
                    None
                }
            })
        })?;
        let mut image = loaded.image;
        let modules = modules::Modules::new(image.image_base, loaded.providers);
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
            startup,
            modules,
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

    /// each guest instruction and each completed api call costs one budget unit.
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
        let argument = frame[1];
        match api {
            Api::ExceptionProlog => {
                return crt::enter_exception_frame(&mut self.cpu, &mut self.memory, frame[0]);
            }
            Api::SetLastError => thread::set_last_error(&mut self.memory, argument)?,
            Api::GetLastError => self.cpu.set_register(Register32::Eax, self.last_error()?),
            Api::ExitProcess => {
                self.exit_code = Some(argument);
                return Ok(());
            }
            Api::GetDesktopWindow => self.cpu.set_register(Register32::Eax, d3d8::DESKTOP),
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
            Api::CodePage(call) => {
                let value = call.dispatch(&frame[1..words], &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Tls(call) => {
                let value = self
                    .tls
                    .dispatch(call, &frame[1..words], &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::CriticalSection(call) => {
                if let Some(value) =
                    self.critical_sections
                        .dispatch(call, argument, &mut self.memory)?
                {
                    self.cpu.set_register(Register32::Eax, value);
                }
            }
            Api::Heap(call) => {
                let value = self
                    .heap
                    .dispatch(call, &frame[1..words], stack, &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Module(call) => {
                let value = self.modules.dispatch(
                    call,
                    argument,
                    self.startup.is_complete(),
                    &mut self.memory,
                )?;
                self.cpu.set_register(Register32::Eax, value);
            }
            Api::Graphics(call) => {
                let result = self
                    .graphics
                    .dispatch(call, &frame[1..words], &mut self.memory)?;
                self.cpu.set_register(Register32::Eax, result);
            }
            Api::Crt(call) => {
                if let Some(value) = self.crt.dispatch(
                    call,
                    &frame[1..words],
                    &mut self.cpu,
                    &mut self.memory,
                    &mut self.heap,
                )? {
                    self.cpu.set_register(Register32::Eax, value);
                }
            }
            Api::Unsupported => unreachable!(),
        }
        // an api output may alias the saved return address; heap free protects this frame.
        guest::read_words(&self.memory, stack, &mut frame[..1])?;
        self.cpu
            .set_register(Register32::Esp, stack.wrapping_add(api.stack_cleanup()));
        self.cpu.eip = frame[0];
        Ok(())
    }
}
