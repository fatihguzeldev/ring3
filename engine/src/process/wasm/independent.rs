use super::{REGISTRY, Registry, status};
use crate::{
    abi::{arena::*, x86::*},
    cpu::{
        ExecutionExit,
        runtime::{self, CpuState, Status, numeric::NumericState},
        x86::{CpuPagingError32, SegmentContext, SegmentRegister},
    },
    memory::{Access, AddressSpace, DirtyPages, FaultReason, GuestAddress, MemoryError},
    process::{EngineInstance, HostError},
};

mod cache;
mod exit;
mod instruction;
use cache::CodeCache;

pub(super) struct IndependentSession {
    pub(super) state: Box<CpuState>,
    pub(super) numeric: Box<NumericState>,
    numeric_view: Box<runtime::NumericView>,
    memory_cache: Box<runtime::memory_cache::MemoryCache>,
    pub(super) segments: SegmentContext,
    pub(super) dirty: DirtyPages,
    cache: CodeCache,
    selected: Option<u32>,
    active: bool,
    failed: bool,
    memory_id: u64,
    thread: u32,
    quota: Option<u32>,
    direct_instruction_calls: u64,
}

impl IndependentSession {
    fn new(instance: &EngineInstance) -> Result<Self, HostError> {
        let memory = instance.memory.as_ref().ok_or(HostError::InvalidRequest)?;
        let mut cache = CodeCache::new()?;
        cache.reserve_memory_cache()?;
        Ok(Self {
            state: Box::default(),
            numeric: Box::default(),
            numeric_view: Box::default(),
            memory_cache: Box::new(
                runtime::memory_cache::MemoryCache::new().map_err(|_| HostError::Infrastructure)?,
            ),
            segments: instance.segment_context()?,
            dirty: DirtyPages::new(memory.capacity_pages()).map_err(HostError::Memory)?,
            cache,
            selected: None,
            active: false,
            failed: false,
            memory_id: memory.identity(),
            thread: instance.threads.current_id(),
            quota: None,
            direct_instruction_calls: 0,
        })
    }

    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    fn pointer(&self) -> u32 {
        std::ptr::from_ref(&*self.state) as usize as u32
    }

    fn guard(&self, instance: &EngineInstance) -> Result<(), HostError> {
        if !self.active
            || self.failed
            || !instance.is_open()
            || instance
                .memory
                .as_ref()
                .is_none_or(|memory| memory.identity() != self.memory_id)
            || instance.threads.current_id() != self.thread
            || self.state.version != runtime::ABI_VERSION
            || self.state.size != runtime::ABI_SIZE
        {
            return Err(HostError::InvalidArtifact);
        }
        Ok(())
    }

    pub(super) fn flush(&mut self, memory: &mut AddressSpace) -> Result<(), HostError> {
        self.memory_cache.invalidate();
        self.publish_dirty_preserving_memory(memory)
    }

    // dirty publication changes content versions and queue state, not translation authority.
    pub(super) fn publish_dirty_preserving_memory(
        &mut self,
        memory: &mut AddressSpace,
    ) -> Result<(), HostError> {
        self.dirty.len = self.state.dirty_len;
        if let Err(error) = self.dirty.publish(memory) {
            self.memory_cache.invalidate();
            return Err(HostError::Memory(error));
        }
        if memory.transaction_version() == u64::MAX {
            self.memory_cache.invalidate();
        }
        self.state.dirty_len = 0;
        Ok(())
    }

    pub(super) fn refresh(&mut self, memory: &mut AddressSpace) {
        self.memory_cache.invalidate();
        self.refresh_preserving_memory(memory);
    }

    pub(super) fn refresh_preserving_memory(&mut self, memory: &mut AddressSpace) {
        self.state.memory_cache = std::ptr::from_ref(&*self.memory_cache) as usize as u32;
        self.numeric_view.x87_state = std::ptr::from_ref(&self.numeric.x87) as usize as u32;
        self.numeric_view.selectors = self.segments.state.selectors.map(u32::from);
        self.numeric_view.simd_state =
            std::ptr::from_ref(&self.numeric.vectors.simd) as usize as u32;
        self.state.numeric = std::ptr::from_ref(&*self.numeric_view) as usize as u32;
        let view = memory.core_view();
        self.state.mappings = view.mappings as u32;
        self.state.permissions = view.permissions as u32;
        self.state.backing = view.backing as u32;
        self.state.backing_pages = view.backing_pages;
        self.state.executable_views = view.executable_views as u32;
        self.state.private_slots = view.private_slots as u32;
        self.state.transaction_version = view.transaction_version as u32;
        self.state.mapping_epoch = memory.transaction_version();
        self.state.code_epoch = memory.code_validation_version();
        self.state.dirty_bits = self.dirty.bits.as_mut_ptr() as usize as u32;
        self.state.dirty_slots = self.dirty.slots.as_mut_ptr() as usize as u32;
        self.state.dirty_capacity = self.dirty.slots.len() as u32;
        self.state.dirty_len = self.dirty.len;
        self.state.directory = self.cache.directory_ptr();
        self.state.directory_mask = self.cache.directory_mask();
        self.state.segment_mask = self
            .segments
            .caches
            .iter()
            .enumerate()
            .fold(0, |mask, (index, cache)| {
                mask | (u32::from(cache.usable()) << index)
            });
    }
}

fn access(
    low: u32,
    high: u32,
    operation: impl FnOnce(&mut EngineInstance, &mut IndependentSession) -> Result<(), HostError>,
) -> u32 {
    let key = u64::from(low) | u64::from(high) << 32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry {
            instance,
            independent,
            ..
        } = &mut *registry;
        let (Some(instance), Some(session)) = (instance.as_mut(), independent.as_mut()) else {
            return 5;
        };
        if instance.key() != key {
            return 3;
        }
        if let Err(error) = session.guard(instance) {
            return status(error);
        }
        operation(instance, session).map_or_else(status, |()| 0)
    })
}

pub(crate) fn cpu_abi() -> u32 {
    runtime::ABI_VERSION
}

pub(crate) fn cpu_begin(low: u32, high: u32) -> u32 {
    let key = u64::from(low) | u64::from(high) << 32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.core.is_some()
            || registry
                .independent
                .as_ref()
                .is_some_and(IndependentSession::is_active)
        {
            return 12;
        }
        let Registry {
            instance,
            independent,
            ..
        } = &mut *registry;
        let Some(instance) = instance.as_mut().filter(|instance| instance.is_open()) else {
            return 5;
        };
        if instance.key() != key {
            return 3;
        }
        let mut begin = || -> Result<(), HostError> {
            #[cfg(feature = "external-cpu")]
            if instance.external_cpu_memory_pinned {
                return Err(HostError::Call(crate::process::CallError::Busy));
            }
            instance.enable_cpu_execution()?;
            instance.guard_cpu_entry()?;
            instance.guard_detection_material_change()?;
            if instance.execution_frame.is_some() {
                return Err(HostError::InvalidRequest);
            }
            if independent.is_none() {
                *independent = Some(IndependentSession::new(instance)?);
            }
            let session = independent.as_mut().unwrap();
            if session.failed {
                return Err(HostError::Infrastructure);
            }
            let arena = instance.arena();
            let scalar = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .map_err(|_| HostError::InvalidRequest)?;
            let metadata = decode_state_metadata(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .map_err(|_| HostError::InvalidRequest)?;
            if metadata.ss_debug_shadow || scalar.eflags & 0x50100 != 0 {
                return Err(HostError::InvalidRequest);
            }
            session.numeric.x87 = decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::InvalidRequest)?;
            session.numeric.vectors.simd =
                decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                    .map_err(|_| HostError::InvalidRequest)?;
            session.numeric.vectors.upper = decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
                .map_err(|_| HostError::InvalidRequest)?
                .registers;
            session.segments = instance.segment_context()?;
            if !runtime::supports_segments(&session.segments) {
                return Err(HostError::InvalidRequest);
            }
            session.state.set_state(scalar);
            session.state.retired = 0;
            session.state.status = 0;
            session.state.detail = 0;
            session.state.vector = 0;
            session.state.auxiliary = 0;
            session.state.fs_base = session.segments.state.bases[SegmentRegister::Fs.index()];
            session.state.gs_base = session.segments.state.bases[SegmentRegister::Gs.index()];
            session.thread = instance.threads.current_id();
            session.quota = None;
            session.selected = None;
            let memory = instance.memory.as_mut().unwrap();
            if session.memory_id != memory.identity() {
                return Err(HostError::InvalidArtifact);
            }
            session.cache.refresh(memory)?;
            session.refresh(memory);
            session.active = true;
            Ok(())
        };
        begin().map_or_else(status, |()| 0)
    })
}

pub(crate) fn cpu_context_ptr() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .independent
                    .as_ref()
                    .map(IndependentSession::pointer)
            })
            .unwrap_or(0)
    })
}

pub(crate) fn cpu_prepare(low: u32, high: u32, pc: u32, maximum: u32) -> u32 {
    let mut stopped = false;
    let result = access(low, high, |instance, session| {
        if maximum == 0 || maximum > 4096 || session.state.eip != pc || session.state.remaining == 0
        {
            return Err(HostError::InvalidRequest);
        }
        session.quota.get_or_insert(session.state.remaining);
        session.selected = None;
        let gates: Vec<_> = instance
            .module_gates()?
            .iter()
            .map(|gate| (gate.entry, gate.id))
            .collect();
        let memory = instance.memory.as_mut().unwrap();
        session.flush(memory)?;
        session.cache.refresh(memory)?;
        if let Some(slot) = session.cache.lookup(pc) {
            session.selected = Some(slot);
            session.refresh(memory);
            return Ok(());
        }
        let binding = session.memory_cache.binding(&session.state, &session.dirty);
        let code = match runtime::compile_with_native_memory(
            memory,
            GuestAddress(pc),
            maximum as usize,
            &gates,
            binding,
            |address| session.cache.lookup(address.0).is_some(),
        ) {
            Ok(code) => code,
            Err(runtime::CompileError::Decode(error)) => {
                instruction::decode_failure(&mut session.state, error)?;
                session.selected = None;
                stopped = true;
                session.refresh(memory);
                return Ok(());
            }
            Err(runtime::CompileError::Memory(error)) => {
                memory_failure(&mut session.state, error, 2);
                session.selected = None;
                stopped = true;
                session.refresh(memory);
                return Ok(());
            }
            Err(_) => return Err(HostError::InvalidArtifact),
        };
        let slot = session.cache.install(code, memory)?;
        session.selected = Some(slot);
        session.refresh(memory);
        Ok(())
    });
    if result == 0 && stopped { 20 } else { result }
}

fn inspect_session(operation: impl FnOnce(&IndependentSession) -> u32) -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| registry.independent.as_ref().map(operation))
            .unwrap_or(0)
    })
}
pub(crate) fn cpu_code_ptr() -> u32 {
    inspect_session(|session| {
        session
            .selected
            .and_then(|slot| session.cache.selected(slot))
            .map_or(0, |code| code.wasm.as_ptr() as usize as u32)
    })
}
pub(crate) fn cpu_code_len() -> u32 {
    inspect_session(|session| {
        session
            .selected
            .and_then(|slot| session.cache.selected(slot))
            .map_or(0, |code| code.wasm.len() as u32)
    })
}
pub(crate) fn cpu_unit_slot() -> u32 {
    inspect_session(|session| session.selected.unwrap_or(0))
}
pub(crate) fn cpu_dispatch_code_ptr() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 0;
        };
        if registry.core.is_some() {
            return 0;
        }
        let Registry {
            instance,
            independent,
            ..
        } = &mut *registry;
        if independent.is_none() {
            let Some(instance) = instance.as_ref() else {
                return 0;
            };
            let Ok(session) = IndependentSession::new(instance) else {
                return 0;
            };
            *independent = Some(session);
        }
        independent.as_ref().unwrap().cache.dispatcher_ptr()
    })
}
pub(crate) fn cpu_dispatch_code_len() -> u32 {
    inspect_session(|session| session.cache.dispatcher_len())
}

pub(crate) fn cpu_cache_bytes() -> u32 {
    inspect_session(|session| {
        (session.cache.charged_bytes() + runtime::memory_cache::RESIDENT_BYTES) as u32
    })
}

pub(crate) fn cpu_cache_victim() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow_mut()
            .ok()
            .and_then(|mut registry| {
                registry
                    .independent
                    .as_mut()
                    .map(|session| session.cache.take_victim())
            })
            .unwrap_or(0)
    })
}

pub(super) fn memory_failure(state: &mut CpuState, error: MemoryError, width: u32) {
    if let MemoryError::Fault(fault) = error {
        state.status = Status::MemoryFault as u32;
        state.detail = fault.address.0;
        let access = match fault.access {
            Access::Read => 1,
            Access::Write => 2,
            Access::Execute => 4,
        };
        let reason = match fault.reason {
            FaultReason::Unmapped => 1,
            FaultReason::Permission => 2,
            FaultReason::AddressOverflow => 3,
        };
        state.vector = width;
        state.auxiliary = (width & 255) | access << 8 | reason << 16;
    } else {
        state.status = Status::Infrastructure as u32;
    }
}

pub(super) fn paging_failure(state: &mut CpuState, error: CpuPagingError32, width: usize) {
    match error {
        CpuPagingError32::Memory(error) => memory_failure(state, error, width as u32),
        CpuPagingError32::PageFault(fault) => {
            state.status = Status::MemoryFault as u32;
            state.detail = fault.linear().0;
            let access = match fault.access() {
                crate::cpu::x86::PagingAccess32::Read => 1,
                crate::cpu::x86::PagingAccess32::Write => 2,
                crate::cpu::x86::PagingAccess32::Execute => 4,
            };
            state.vector = width as u32;
            state.auxiliary = (width as u32 & 255)
                | access << 8
                | (if fault.error_code() & 1 != 0 { 2 } else { 1 }) << 16;
        }
    }
}

fn helper<const INVALIDATES_MEMORY: bool>(
    context: u32,
    operation: impl FnOnce(&mut EngineInstance, &mut IndependentSession) -> u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return Status::Infrastructure as u32;
        };
        let Registry {
            instance,
            independent,
            ..
        } = &mut *registry;
        let (Some(instance), Some(session)) = (instance.as_mut(), independent.as_mut()) else {
            return Status::Infrastructure as u32;
        };
        if session.pointer() != context || session.guard(instance).is_err() {
            session.state.status = Status::Infrastructure as u32;
            return Status::Infrastructure as u32;
        }
        if INVALIDATES_MEMORY {
            session.memory_cache.invalidate();
        }
        operation(instance, session)
    })
}

pub(crate) fn cpu_runtime_read(context: u32, address: u32, width: u32) -> u32 {
    helper::<false>(context, |instance, session| {
        let memory = instance.memory.as_ref().unwrap();
        session.state.status = 0;
        match memory.core_read(address, width) {
            Ok(value) => value,
            Err(error) => {
                memory_failure(&mut session.state, error, width & 255);
                0
            }
        }
    })
}

pub(crate) fn cpu_runtime_write(context: u32, address: u32, width: u32, value: u32) -> u32 {
    helper::<true>(context, |instance, session| {
        let memory = instance.memory.as_mut().unwrap();
        session.dirty.len = session.state.dirty_len;
        session.state.status = match memory.runtime_store(&mut session.dirty, address, width, value)
        {
            Ok(false) => 0,
            Ok(true) => Status::CodeInvalidated as u32,
            Err(error) => {
                memory_failure(&mut session.state, error, width);
                session.state.status
            }
        };
        session.refresh(memory);
        session.state.status
    })
}

pub(crate) fn cpu_instruction(low: u32, high: u32) -> u32 {
    access(low, high, instruction::execute)
}

pub(crate) fn cpu_runtime_instruction(context: u32) -> u32 {
    // instruction execution classifies memory effects before invoking semantics.
    helper::<false>(context, |instance, session| {
        let Some(attempts) = session.direct_instruction_calls.checked_add(1) else {
            session.memory_cache.invalidate();
            session.state.status = Status::Infrastructure as u32;
            session.failed = true;
            return Status::Infrastructure as u32;
        };
        session.direct_instruction_calls = attempts;
        if instruction::execute(instance, session).is_err() {
            session.state.status = Status::Infrastructure as u32;
            session.failed = true;
        }
        session.state.status
    })
}

pub(crate) fn cpu_runtime_instruction_count_low() -> u32 {
    inspect_session(|session| session.direct_instruction_calls as u32)
}

pub(crate) fn cpu_runtime_instruction_count_high() -> u32 {
    inspect_session(|session| (session.direct_instruction_calls >> 32) as u32)
}

pub(crate) fn cpu_publish(low: u32, high: u32) -> u32 {
    access(low, high, |instance, session| {
        if session.state.status == Status::Gate as u32 {
            if instance
                .module_gate_at(session.state.eip)
                .is_none_or(|gate| gate.id != session.state.vector)
            {
                return Err(HostError::InvalidArtifact);
            }
            let mut stub = [0; 2];
            instance
                .memory
                .as_ref()
                .unwrap()
                .fetch(GuestAddress(session.state.eip), &mut stub)
                .map_err(HostError::Memory)?;
            if stub != [0x0f, 0x0b] {
                return Err(HostError::InvalidArtifact);
            }
        }
        let memory = instance.memory.as_mut().unwrap();
        session.flush(memory)?;
        let work = session
            .quota
            .unwrap_or(session.state.remaining)
            .checked_sub(session.state.remaining)
            .ok_or(HostError::InvalidRequest)?;
        if session.state.retired > work {
            return Err(HostError::InvalidRequest);
        }
        let (reason, origin, instruction) = exit::decode(&session.state, work)?;
        let upper = crate::cpu::x86::simd::ymm::YmmUpperState {
            registers: session.numeric.vectors.upper,
        };
        let prepared = instance.prepare_cpu_exit(
            session.state.state(),
            ExecutionExit {
                retired: session.state.retired,
                reason,
            },
            work,
            origin,
            instruction,
            (
                session.numeric.x87,
                session.numeric.vectors.simd,
                upper,
                session.segments,
            ),
        )?;
        let arena = instance.arena.as_mut().get_mut();
        encode_x87(
            &session.numeric.x87,
            &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        encode_simd(
            &session.numeric.vectors.simd,
            &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        instance.commit_cpu_exit(prepared);
        session.active = false;
        Ok(())
    })
}

pub(crate) fn cpu_end(low: u32, high: u32) -> u32 {
    let key = u64::from(low) | u64::from(high) << 32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry {
            instance,
            independent,
            ..
        } = &mut *registry;
        let (Some(instance), Some(session)) = (instance.as_mut(), independent.as_mut()) else {
            return 5;
        };
        if instance.key() != key {
            return 3;
        }
        let result = session.flush(instance.memory.as_mut().unwrap());
        session.active = false;
        session.failed = true;
        result.map_or_else(status, |()| 0)
    })
}

pub(crate) fn cpu_capture_windows_call() -> u32 {
    super::mutate(|instance| instance.capture_cpu_windows_call().map(|_| ()))
}
pub(crate) fn cpu_complete_windows_call(token: u32) -> u32 {
    super::mutate(|instance| instance.complete_cpu_windows_call(token))
}
pub(crate) fn cpu_begin_windows_callback(return_pc: u32, return_id: u32) -> u32 {
    super::mutate(|instance| {
        instance
            .begin_cpu_windows_callback(return_pc, return_id)
            .map(|_| ())
    })
}
pub(crate) fn cpu_finish_windows_callback(token: u32) -> u32 {
    super::mutate(|instance| instance.finish_cpu_windows_callback(token).map(|_| ()))
}

pub(crate) fn cpu_dispatch_exception() -> u32 {
    let identity = REGISTRY.with(|registry| {
        registry.try_borrow().ok().and_then(|registry| {
            let instance = registry.instance.as_ref()?;
            Some((instance.key(), instance.cpu_event.as_ref()?.epoch))
        })
    });
    let Some((key, epoch)) = identity else {
        return 7;
    };
    super::dispatch_windows_exception(
        key as u32,
        (key >> 32) as u32,
        epoch as u32,
        (epoch >> 32) as u32,
    )
}

pub(crate) fn cpu_finish_exception() -> u32 {
    let identity = REGISTRY.with(|registry| {
        registry.try_borrow().ok().and_then(|registry| {
            let instance = registry.instance.as_ref()?;
            Some((instance.key(), instance.pending_windows_exception()?.id))
        })
    });
    let Some((key, id)) = identity else { return 7 };
    super::finish_windows_exception_handler(
        key as u32,
        (key >> 32) as u32,
        id as u32,
        (id >> 32) as u32,
    )
}
