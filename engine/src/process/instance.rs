#![forbid(unsafe_code)]

use std::pin::Pin;

use crate::process::calls::call::{CallError, PendingCall};
use crate::process::calls::callback::SuspendedCallback;
use crate::process::execution::installation::{RESIDENT_INSTALLATION_SLOTS, ResidentInstallation};

use crate::{
    abi::arena::{self, ARENA_SIZE},
    cpu::dbt::{CompileError, CompiledRegion, RegistryError, ResidentRegistry, emit_dispatcher},
    memory::{AddressSpace, MemoryError, PageRange},
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
    Compile(CompileError),
    Resident(RegistryError),
    Infrastructure,
    Call(CallError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreCompletion {
    Complete,
    CodeInvalidated,
}

pub struct EngineInstance {
    pub(in crate::process) memory: Option<AddressSpace>,
    pub(in crate::process) arena: Pin<Box<[u8]>>,
    pub(in crate::process) artifact: Option<CompiledRegion>,
    pub(in crate::process) dispatcher: Option<Vec<u8>>,
    pub(in crate::process) resident: Option<ResidentRegistry>,
    pub(in crate::process) resident_installations:
        [Option<ResidentInstallation>; RESIDENT_INSTALLATION_SLOTS],
    pub(in crate::process) key: u64,
    pub(in crate::process) generation: u32,
    pub(in crate::process) pending_call: Option<PendingCall>,
    pub(in crate::process) callback: Option<SuspendedCallback>,
    pub(in crate::process) call_token: u32,
    pub(in crate::process) cpu_event: Option<crate::process::CpuEvent32>,
    pub(in crate::process) virtual_tsc: u64,
    pub(in crate::process) cr2: Option<crate::memory::GuestAddress>,
    pub(in crate::process) cpu_page_fault_sequence: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) detection_epoch: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) exception_detection:
        Option<Box<[crate::process::cpu::interrupts::detection::DetectionSlot32; 1]>>,
    pub(in crate::process) system_register_bank: Option<crate::cpu::x86::UserSystemRegisterBank32>,
    pub(in crate::process) image: Option<crate::loader::ImageMetadata32>,
    pub(in crate::process) image_input: Option<crate::process::loading::input::ImageInput>,
    pub(in crate::process) image_started: bool,
    pub(in crate::process) exit_code: Option<u32>,
    pub(in crate::process) windows_thread: crate::windows::ThreadState32,
    pub(in crate::process) segment_descriptors: crate::cpu::x86::SegmentDescriptorTable,
    pub(in crate::process) virtual_allocations: Vec<PageRange>,
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
            memory: Some(memory),
            arena: Box::into_pin(arena.into_boxed_slice()),
            artifact: None,
            dispatcher: Some(emit_dispatcher(key)),
            resident: None,
            resident_installations: [None; RESIDENT_INSTALLATION_SLOTS],
            key,
            generation: 0,
            pending_call: None,
            callback: None,
            call_token: 0,
            cpu_event: None,
            virtual_tsc: 0,
            cr2: None,
            cpu_page_fault_sequence: 0,
            #[cfg(any(target_arch = "wasm32", test))]
            detection_epoch: 0,
            #[cfg(any(target_arch = "wasm32", test))]
            exception_detection: None,
            system_register_bank: Some(crate::cpu::x86::UserSystemRegisterBank32::application()),
            image: None,
            image_input: None,
            image_started: false,
            exit_code: None,
            windows_thread: crate::windows::ThreadState32::default(),
            segment_descriptors: crate::cpu::x86::SegmentDescriptorTable::default(),
            virtual_allocations: Vec::new(),
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
        self.image_input = None;
        self.system_register_bank = None;
        self.cr2 = None;
        self.cpu_page_fault_sequence = 0;
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.exception_detection = None;
        }
        self.pending_call = None;
        self.cpu_event = None;
        self.callback = None;
        self.artifact = None;
        self.dispatcher = None;
        self.resident = None;
        self.resident_installations = [None; RESIDENT_INSTALLATION_SLOTS];
        self.memory = None;
        self.virtual_allocations = Vec::new();
    }
}
