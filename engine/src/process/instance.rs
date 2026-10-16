#![forbid(unsafe_code)]

use std::pin::Pin;

use crate::process::calls::call::{CallError, PendingCall};
use crate::process::calls::callback::SuspendedCallback;
use crate::process::execution::installation::{RESIDENT_INSTALLATION_SLOTS, ResidentInstallation};

use crate::{
    abi::arena::{self, ARENA_SIZE},
    cpu::dbt::{CompileError, CompiledRegion, RegistryError, ResidentRegistry, emit_dispatcher},
    memory::{AddressSpace, MemoryError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    Closed,
    ProcessExited,
    InvalidRequest,
    InvalidArtifact,
    CodeInvalidated,
    GenerationExhausted,
    Memory(MemoryError),
    Loader(crate::loader::LoadError),
    Module(crate::loader::ModuleError32),
    Compile(CompileError),
    Resident(RegistryError),
    Infrastructure,
    Call(CallError),
    Media(crate::process::media::MediaError),
    User32(crate::windows::user32::User32Error32),
    D3D8(crate::process::windows::d3d8::D3D8Error32),
    Synchronization(crate::windows::synchronization::SynchronizationError32),
    DSound(crate::process::windows::dsound::DSoundError32),
    DirectInput(crate::process::windows::dinput::DirectInputError32),
    File(crate::files::FileError32),
    Ole32(crate::process::windows::ole32::Ole32Error32),
    WindowsProcess(crate::windows::process::ProcessError32),
    WindowsMemory(crate::windows::memory::MemoryError32),
    Thread(crate::windows::thread::ThreadError32),
    Registry(crate::windows::registry::RegistryError32),
    Exception(crate::windows::exceptions::ExceptionError32),
    Nls(crate::windows::nls::NlsError32),
    Resource(crate::process::windows::resources::ResourceError32),
    Activation(crate::windows::activation::ActivationError32),
    Version(crate::windows::version::VersionError32),
    Shlwapi(crate::windows::shlwapi::ShlwapiError32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreCompletion {
    Complete,
    CodeInvalidated,
}

// Keep scalar memory and arena access at a fixed, small offset as Windows
// services grow. The default Rust layout placed the arena beyond ARM64's
// immediate load range in the generated Wasm hot path.
#[repr(C)]
pub struct EngineInstance {
    pub(in crate::process) memory: Option<AddressSpace>,
    pub(in crate::process) arena: Pin<Box<[u8]>>,
    pub(in crate::process) console: crate::windows::console::ConsoleControl32,
    pub(in crate::process) native_diagnostics: crate::windows::native::NativeDiagnostics32,
    pub(in crate::process) resources: Option<crate::process::windows::resources::ResourceRuntime32>,
    pub(in crate::process) activation:
        Option<crate::process::windows::activation::ActivationRuntime32>,
    pub(in crate::process) exceptions: crate::process::windows::exceptions::ExceptionRuntime32,
    pub(in crate::process) version: crate::process::windows::version::VersionRuntime32,
    pub(in crate::process) nls: crate::process::windows::nls::NlsRuntime32,
    pub(in crate::process) registry: Option<crate::process::windows::registry::RegistryRuntime32>,
    pub(in crate::process) files: Option<crate::process::windows::files::FileRuntime32>,
    pub(in crate::process) artifact: Option<CompiledRegion>,
    pub(in crate::process) dispatcher: Option<Vec<u8>>,
    pub(in crate::process) resident: Option<ResidentRegistry>,
    pub(in crate::process) resident_installations: Vec<Option<ResidentInstallation>>,
    pub(in crate::process) execution_cache:
        Option<crate::process::execution::cache::ExecutionCache32>,
    pub(in crate::process) key: u64,
    pub(in crate::process) generation: u32,
    pub(in crate::process) generation_issuer: u32,
    pub(in crate::process) pending_call: Option<PendingCall>,
    pub(in crate::process) module_windows_reply:
        Option<crate::process::windows::modules::ModuleWindowsReply>,
    pub(in crate::process) callback: Option<SuspendedCallback>,
    pub(in crate::process) call_token: u32,
    pub(in crate::process) cpu_event: Option<crate::process::CpuEvent32>,
    pub(in crate::process) numerical_workspace: crate::math::NumericalWorkspace,
    pub(in crate::process) cpu_clock: crate::process::clock::CpuClock,
    pub(in crate::process) cr2: Option<crate::memory::GuestAddress>,
    pub(in crate::process) cpu_page_fault_sequence: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) detection_epoch: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) exception_detection:
        Option<Box<[crate::process::cpu::events::detection::DetectionSlot32; 1]>>,
    pub(in crate::process) system_register_bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
    pub(in crate::process) modules: crate::process::loading::modules::ModuleProcess32,
    pub(in crate::process) image: Option<crate::loader::ImageMetadata32>,
    pub(in crate::process) image_input: Option<crate::process::loading::input::ImageInput>,
    pub(in crate::process) image_started: bool,
    pub(in crate::process) exit_code: Option<u32>,
    pub(in crate::process) pending_process_exit: Option<u32>,
    pub(in crate::process) process_exit_owner: Option<u32>,
    pub(in crate::process) threads: crate::process::windows::threads::ThreadRuntime32,
    pub(in crate::process) process_runtime:
        crate::process::windows::process::WindowsProcessRuntime32,
    pub(in crate::process) windows_thread: crate::windows::ThreadState32,
    pub(in crate::process) windows_service:
        Option<crate::process::windows::service::WindowsService32>,
    pub(in crate::process) user32: Option<crate::process::windows::user32::User32Context32>,
    pub(in crate::process) windows_replies: Vec<crate::process::windows::callbacks::WindowsReply32>,
    pub(in crate::process) segment_descriptors: crate::cpu::x86::SegmentDescriptorTable,
    pub(in crate::process) windows_memory: crate::process::windows::memory::WindowsMemoryRuntime32,
    pub(in crate::process) windows_security: crate::windows::security::SecurityRuntime32,
    pub(in crate::process) media: Option<crate::process::media::MediaContexts>,
    pub(in crate::process) media_epoch: u64,
    pub(in crate::process) windows_input_delivery_error: Option<HostError>,
    pub(in crate::process) d3d8: Option<crate::process::windows::d3d8::D3D8Runtime32>,
    pub(in crate::process) d3d8_reply: Option<crate::process::windows::d3d8::D3D8Reply32>,
    pub(in crate::process) synchronization:
        Option<crate::process::windows::synchronization::SynchronizationRuntime32>,
    pub(in crate::process) dsound: Option<crate::process::windows::dsound::DSoundRuntime32>,
    pub(in crate::process) dinput: Option<crate::process::windows::dinput::DirectInputRuntime32>,
    pub(in crate::process) ole32: Option<crate::process::windows::ole32::Ole32Runtime32>,
    pub(in crate::process) ole32_task_memory: crate::process::windows::ole32::Ole32TaskMemory32,
    pub(in crate::process) winmm: crate::process::windows::winmm::WinmmRuntime32,
}

impl EngineInstance {
    pub fn new(pages: u32, key: u64) -> Result<Self, HostError> {
        if pages == 0 || pages > 4096 || key == 0 {
            return Err(HostError::InvalidRequest);
        }
        let memory = AddressSpace::new(pages).map_err(HostError::Memory)?;
        let mut arena = Vec::new();
        arena
            .try_reserve_exact(ARENA_SIZE)
            .map_err(|_| HostError::Infrastructure)?;
        arena.resize(ARENA_SIZE, 0);
        arena::initialize(&mut arena).map_err(|_| HostError::Infrastructure)?;
        Ok(Self {
            console: crate::windows::console::ConsoleControl32::new(),
            native_diagnostics: Default::default(),
            activation: None,
            windows_security: Default::default(),
            resources: None,
            registry: None,
            exceptions: crate::process::windows::exceptions::ExceptionRuntime32::new()
                .map_err(HostError::Exception)?,
            version: Default::default(),
            nls: Default::default(),
            files: None,
            memory: Some(memory),
            arena: Box::into_pin(arena.into_boxed_slice()),
            artifact: None,
            dispatcher: Some(emit_dispatcher(key)),
            resident: None,
            resident_installations: vec![None; RESIDENT_INSTALLATION_SLOTS],
            execution_cache: None,
            key,
            generation: 0,
            generation_issuer: 0,
            pending_call: None,
            module_windows_reply: None,
            callback: None,
            call_token: 0,
            cpu_event: None,
            numerical_workspace: Default::default(),
            cpu_clock: crate::process::clock::CpuClock::default(),
            cr2: None,
            cpu_page_fault_sequence: 0,
            #[cfg(any(target_arch = "wasm32", test))]
            detection_epoch: 0,
            #[cfg(any(target_arch = "wasm32", test))]
            exception_detection: None,
            system_register_bank: Some(crate::cpu::x86::UserSystemRegisterBank32::application()),
            modules: crate::process::loading::modules::ModuleProcess32::default(),
            image: None,
            image_input: None,
            image_started: false,
            exit_code: None,
            pending_process_exit: None,
            process_exit_owner: None,
            threads: crate::process::windows::threads::ThreadRuntime32::new(Default::default())
                .map_err(HostError::Thread)?,
            process_runtime: crate::process::windows::process::WindowsProcessRuntime32::new(
                Default::default(),
            )
            .map_err(HostError::WindowsProcess)?,
            windows_thread: crate::windows::ThreadState32::default(),
            windows_service: None,
            user32: None,
            windows_replies: Vec::new(),
            segment_descriptors: crate::cpu::x86::SegmentDescriptorTable::default(),
            windows_memory: crate::process::windows::memory::WindowsMemoryRuntime32::new(
                Default::default(),
            )
            .map_err(HostError::WindowsMemory)?,
            media: None,
            media_epoch: 0,
            windows_input_delivery_error: None,
            d3d8: None,
            d3d8_reply: None,
            synchronization: None,
            dsound: None,
            dinput: None,
            ole32: None,
            ole32_task_memory: Default::default(),
            winmm: Default::default(),
        })
    }

    pub fn is_open(&self) -> bool {
        self.memory.is_some()
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn arena(&self) -> &[u8] {
        self.arena.as_ref().get_ref()
    }

    pub fn arena_mut(&mut self) -> Result<&mut [u8], HostError> {
        self.memory()?;
        Ok(self.arena.as_mut().get_mut())
    }

    pub fn arena_address(&self) -> usize {
        self.arena.as_ptr() as usize
    }

    pub fn close(&mut self) {
        self.console = crate::windows::console::ConsoleControl32::new();
        self.native_diagnostics = Default::default();
        self.activation = None;
        self.windows_security = Default::default();
        self.windows_service = None;
        if let Some(memory) = self.memory.as_ref() {
            memory.end_guard_service();
        }
        self.resources = None;
        self.registry = None;
        self.exceptions = crate::process::windows::exceptions::ExceptionRuntime32::new()
            .expect("default exception handler limit");
        self.version = Default::default();
        self.nls = Default::default();
        self.files = None;
        self.d3d8 = None;
        self.d3d8_reply = None;
        self.synchronization = None;
        self.dsound = None;
        self.dinput = None;
        self.ole32 = None;
        self.ole32_task_memory = Default::default();
        self.winmm = Default::default();
        self.execution_cache = None;
        self.numerical_workspace = Default::default();
        self.media = None;
        self.windows_input_delivery_error = None;
        self.cpu_clock = crate::process::clock::CpuClock::default();
        self.image_input = None;
        self.modules = crate::process::loading::modules::ModuleProcess32::default();
        self.system_register_bank = None;
        self.cr2 = None;
        self.cpu_page_fault_sequence = 0;
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.exception_detection = None;
        }
        self.pending_call = None;
        self.module_windows_reply = None;
        self.user32 = None;
        self.windows_replies = Vec::new();
        self.cpu_event = None;
        self.callback = None;
        self.artifact = None;
        self.dispatcher = None;
        self.resident = None;
        self.resident_installations.clear();
        self.memory = None;
        self.windows_memory =
            crate::process::windows::memory::WindowsMemoryRuntime32::new(Default::default())
                .expect("default Windows memory limits are valid");
        self.process_runtime.startup = None;
        self.threads.reset_primary_thread();
    }
}
