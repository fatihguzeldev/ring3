mod boundary;
mod cache;
mod cold;
mod dispatch;
mod numeric;
mod retention;
mod tracing;
pub(crate) use tracing::{
    core_trace_finish, core_trace_pointer, core_trace_ready, core_trace_root, core_trace_start,
    core_trace_status_ptr,
};

pub(crate) use cold::{core_cold_run, core_precise};
pub(crate) use dispatch::{
    core_chain_cold, core_chain_next, core_chain_probe, core_chain_select, core_dispatch_code_len,
    core_dispatch_code_ptr, core_link, core_unlink,
};
#[cfg(feature = "observability")]
pub(crate) use retention::core_cold_status_ptr;
pub(crate) use retention::{
    core_cache_format, core_cache_status_ptr, core_cache_victim, core_link_owners_ptr,
};

pub(crate) use boundary::{
    core_begin_windows_callback, core_capture_windows_call, core_complete_windows_call,
    core_dispatch_exception, core_export, core_finish_exception, core_finish_windows_callback,
    core_import, core_numeric,
};

use super::{REGISTRY, Registry, status};
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, USER_FLAGS_MASK_V3, decode_state, decode_state_metadata},
    },
    cpu::{
        core::{
            CompileError, CompileOptions, Context32, cold::ColdState, compile_with_frontier_query,
        },
        x86::{SegmentContext, SegmentRegister},
    },
    memory::{Access, CoreWriteOutcome, FaultReason, GuestAddress, MemoryError},
    process::{CallError, EngineInstance, HostError, cpu::events::detection::DetectionSlot32},
};
use cache::{CoreCache32, OwnedEntryLookup, RegionRef};
use numeric::NumericBank;

pub(super) struct CoreSession32 {
    code: Option<RegionRef>,
    cache: CoreCache32,
    cache_status: [u64; 12],
    #[cfg(feature = "observability")]
    cold_status: [u64; 13],
    numeric: Box<NumericBank>,
    exported_retired: u32,
    context: Box<Context32>,
    active: bool,
    cold_state: ColdState,
    tracing: tracing::TraceController,
}

impl CoreSession32 {
    fn from_instance(instance: &EngineInstance) -> Result<Self, HostError> {
        let state = decode_state(&instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        Ok(Self {
            code: None,
            cache: CoreCache32::default(),
            cache_status: [0; 12],
            #[cfg(feature = "observability")]
            cold_status: [0; 13],
            numeric: Box::new(NumericBank::from_instance(instance)?),
            exported_retired: 0,
            context: Box::new(Context32::new(state)),
            active: false,
            cold_state: ColdState::default(),
            tracing: tracing::TraceController::default(),
        })
    }

    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    fn context_address(&mut self) -> usize {
        self.context.bytes_mut().as_mut_ptr() as usize
    }

    fn begin(&mut self, instance: &mut EngineInstance, compiled: bool) -> Result<(), u32> {
        let segments = admission(instance).map_err(status)?;
        self.begin_with_segments(instance, compiled, segments)
    }

    fn begin_with_segments(
        &mut self,
        instance: &mut EngineInstance,
        compiled: bool,
        segments: SegmentContext,
    ) -> Result<(), u32> {
        let code = if compiled {
            Some(self.code.as_ref().ok_or(7_u32)?)
        } else {
            None
        };
        let memory = instance.memory.as_mut().ok_or(5_u32)?;
        if code.is_some_and(|code| !self.cache.precise_source_current(memory, code)) {
            return Err(4);
        }
        let flags = self.context.state().eflags;
        if flags & 2 == 0 || flags & !USER_FLAGS_MASK_V3 != 0 || flags & 0x50100 != 0 {
            return Err(7);
        }
        let identity = code.map_or(0, |code| code.identity());
        let view = memory.core_view();
        for (offset, value) in [
            (56, view.mappings),
            (60, view.permissions),
            (64, view.backing),
            (68, view.backing_pages as usize),
            (72, view.executable_views),
            (76, view.private_slots),
            (80, view.transaction_version),
            (84, view.page_versions),
            (88, view.code_validation_version),
        ] {
            self.context.set_word(offset, value as u32);
        }
        self.context.bytes_mut()[96..104]
            .copy_from_slice(&view.expected_code_version.to_le_bytes());
        self.context
            .set_word(104, segments.state.bases[SegmentRegister::Fs.index()]);
        self.context
            .set_word(108, segments.state.bases[SegmentRegister::Gs.index()]);
        self.numeric.refresh_segments(segments);
        self.context
            .set_word(92, (&mut *self.numeric as *mut NumericBank) as usize as u32);
        for offset in [48, 52, 116] {
            self.context.set_word(offset, 0);
        }
        if !compiled {
            self.code = None;
        }
        self.context.bytes_mut()[120..128].copy_from_slice(&identity.to_le_bytes());
        self.context.set_word(112, 1);
        self.active = true;
        Ok(())
    }
}

fn admission(instance: &EngineInstance) -> Result<SegmentContext, HostError> {
    instance.guard_core_entry()?;
    instance.guard_detection_material_change()?;
    if instance.execution_frame.is_some()
        || instance.cpu_event.is_some()
        || instance.exception_detection.as_ref().is_some_and(|slot| {
            !matches!(
                slot[0],
                DetectionSlot32::Idle | DetectionSlot32::Returned(_)
            )
        })
    {
        return Err(HostError::Call(CallError::Busy));
    }
    #[cfg(feature = "external-cpu")]
    if instance.external_cpu_memory_pinned {
        return Err(HostError::Call(CallError::Busy));
    }
    let segments = instance.segment_context()?;
    if !segments.is_legacy()
        || !segments.caches[..4].iter().all(|cache| cache.usable())
        || !segments.caches[SegmentRegister::Fs.index()].usable()
    {
        return Err(HostError::InvalidRequest);
    }
    let state = &instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE];
    if decode_state_metadata(state)
        .map_err(|_| HostError::InvalidRequest)?
        .ss_debug_shadow
    {
        return Err(HostError::InvalidRequest);
    }
    Ok(segments)
}

fn compile_status(error: CompileError) -> u32 {
    match error {
        CompileError::InvalidLimits => 7,
        CompileError::Decode(_) => 20,
        CompileError::Memory(_) => 8,
        CompileError::WasmLimit => 18,
    }
}

pub(crate) fn core_prepare(pc: u32, instructions: u32, blocks: u32) -> u32 {
    if instructions == 0 || instructions > 16384 || blocks == 0 || blocks > 1024 {
        return 7;
    }
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        if core.as_ref().is_some_and(CoreSession32::is_active) {
            return 12;
        }
        if let Some(session) = core.as_mut() {
            session.context.set_word(112, 0);
            session.refresh_trace_bypass(session.context.retired());
        }
        let Some(instance) = instance.as_mut() else {
            return 5;
        };
        if let Err(error) = instance
            .enable_core_execution()
            .and_then(|_| admission(instance))
        {
            return status(error);
        }
        let options = CompileOptions {
            instructions: instructions as usize,
            blocks: blocks as usize,
            ..CompileOptions::default()
        };
        let mut admitted = false;
        let mut port_bound = false;
        let mut transient = false;
        let selected = core
            .as_mut()
            .and_then(|session| session.cache.lookup(instance.memory.as_ref().unwrap(), pc));
        let mut fresh_cache = core.is_none().then(CoreCache32::default);
        let code = if let Some(code) = selected {
            code
        } else {
            let gates = match instance.module_gates() {
                Ok(gates) => gates,
                Err(error) => return status(error),
            };
            let gates: Vec<_> = gates.iter().map(|gate| (gate.entry, gate.id)).collect();
            let memory = instance.memory.as_ref().unwrap();
            let owned = match core.as_mut() {
                Some(session) => match session
                    .cache
                    .prepare_owned_entry(memory, pc, options, &gates)
                {
                    Ok(lookup) => lookup,
                    Err(error) => return compile_status(error),
                },
                None => OwnedEntryLookup::Unowned,
            };
            match owned {
                OwnedEntryLookup::Ready(code) => {
                    port_bound = true;
                    code
                }
                OwnedEntryLookup::Declined => {
                    // A bounded entry upgrade must not create a second retained
                    // owner for this suffix. Run the same exact emitter for one
                    // instruction, without indexing or linking the temporary unit.
                    let code = match compile_with_frontier_query(
                        memory,
                        GuestAddress(pc),
                        CompileOptions {
                            instructions: 1,
                            blocks: 1,
                            ..options
                        },
                        &gates,
                        &|candidate| candidate != pc,
                    ) {
                        Ok(code) => code,
                        Err(error) => return compile_status(error),
                    };
                    transient = true;
                    RegionRef::transient(code)
                }
                OwnedEntryLookup::Unowned => {
                    let frontier = |candidate| {
                        core.as_ref().is_some_and(|session| {
                            session.cache.coverage_contains(memory, candidate)
                        })
                    };
                    let code = match compile_with_frontier_query(
                        instance.memory.as_ref().unwrap(),
                        GuestAddress(pc),
                        options,
                        &gates,
                        &frontier,
                    ) {
                        Ok(code) => {
                            admitted = true;
                            code
                        }
                        Err(error) => return compile_status(error),
                    };
                    let admitted_code = match core.as_mut() {
                        Some(session) => session.cache.admit(memory, code),
                        None => fresh_cache.as_mut().unwrap().admit(memory, code),
                    };
                    match admitted_code {
                        Ok(code) => code,
                        Err(error) => return compile_status(error),
                    }
                }
            }
        };
        let identity = code.identity();
        if let Some(session) = core.as_mut() {
            session.code = Some(code);
        } else {
            let mut state =
                match decode_state(&instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]) {
                    Ok(state) => state,
                    Err(_) => return 7,
                };
            state.eip = pc;
            let numeric = match NumericBank::from_instance(instance) {
                Ok(bank) => bank,
                Err(error) => return status(error),
            };
            *core = Some(CoreSession32 {
                code: Some(code),
                cache: fresh_cache.unwrap(),
                cache_status: [0; 12],
                #[cfg(feature = "observability")]
                cold_status: [0; 13],
                numeric: Box::new(numeric),
                exported_retired: 0,
                context: Box::new(Context32::new(state)),
                active: false,
                cold_state: ColdState::default(),
                tracing: tracing::TraceController::default(),
            });
        }
        let session = core.as_mut().unwrap();
        if admitted {
            session.cold_state.forget_compiled(
                instance.memory.as_ref().unwrap(),
                session.code.as_ref().unwrap().instruction_pcs(),
            );
        } else if port_bound {
            session
                .cold_state
                .forget_compiled(instance.memory.as_ref().unwrap(), [pc]);
        }
        session.context.bytes_mut()[120..128].copy_from_slice(&identity.to_le_bytes());
        session.context.set_word(112, if transient { 2 } else { 0 });
        if transient { 18 } else { 0 }
    })
}

// Select only an entry already admitted to the executable directory. This does
// not acquire a CPU lifetime, compile code, or promote source coverage to an entry.
pub(crate) fn core_lookup(pc: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(session) = core.as_mut() else {
            return if instance.is_some() { 4 } else { 5 };
        };
        if session.active {
            return 12;
        }
        let Some(memory) = instance
            .as_ref()
            .and_then(|instance| instance.memory.as_ref())
        else {
            return 5;
        };
        session.refresh_trace_bypass(session.context.retired());
        let Some(code) = session.cache.lookup(memory, pc) else {
            return 4;
        };
        let identity = code.identity();
        session.code = Some(code);
        session.context.bytes_mut()[120..128].copy_from_slice(&identity.to_le_bytes());
        session.context.set_word(112, 0);
        0
    })
}

pub(crate) fn core_code_ptr() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .core
                    .as_ref()
                    .and_then(|core| core.code.as_ref())
                    .map(|code| code.wasm().as_ptr() as usize as u32)
            })
            .unwrap_or(0)
    })
}

pub(crate) fn core_code_len() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .core
                    .as_ref()
                    .and_then(|core| core.code.as_ref())
                    .map(|code| code.wasm().len() as u32)
            })
            .unwrap_or(0)
    })
}

pub(crate) fn core_context_ptr() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 0;
        };
        let Some(core) = registry.core.as_mut().filter(|core| !core.active) else {
            return 0;
        };
        core.context_address() as u32
    })
}

pub(crate) fn core_begin() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else {
            return 7;
        };
        if core.active {
            return 12;
        }
        let Some(instance) = instance.as_mut() else {
            return 5;
        };
        match core.begin(instance, true) {
            Ok(()) => 0,
            Err(status) => status,
        }
    })
}

pub(crate) fn core_end() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(core) = registry.core.as_mut() else {
            return 7;
        };
        if !core.active {
            return 7;
        }
        core.context.set_word(112, 0);
        core.active = false;
        core.tracing.end();
        core.refresh_trace_bypass(core.context.retired());
        if core.code.as_ref().is_some_and(RegionRef::is_transient) {
            core.code = None;
        }
        0
    })
}

fn memory_failure(core: &mut CoreSession32, error: MemoryError, width: u32) -> u32 {
    let MemoryError::Fault(fault) = error else {
        return 3;
    };
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
    core.context.set_word(116, 14);
    core.context.set_word(48, fault.address.0);
    core.context
        .set_word(52, (width & 0xff) | access << 8 | reason << 16);
    1
}

pub(crate) fn core_read(context: u32, address: u32, width: u32) -> i64 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return (3_i64) << 32;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else {
            return (3_i64) << 32;
        };
        if !core.active || core.context_address() as u32 != context {
            return (3_i64) << 32;
        }
        let Some(memory) = instance
            .as_ref()
            .and_then(|instance| instance.memory.as_ref())
        else {
            return (3_i64) << 32;
        };
        match memory.core_read(address, width) {
            Ok(value) => i64::from(value),
            Err(error) => i64::from(memory_failure(core, error, width)) << 32,
        }
    })
}

pub(crate) fn core_write(context: u32, address: u32, width: u32, value: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 3;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else {
            return 3;
        };
        if !core.active || core.context_address() as u32 != context {
            return 3;
        }
        let Some(memory) = instance
            .as_mut()
            .and_then(|instance| instance.memory.as_mut())
        else {
            return 3;
        };
        match memory.core_write(address, width, value) {
            Ok(CoreWriteOutcome::Complete) => 0,
            Ok(CoreWriteOutcome::CodeInvalidated) => 2,
            Err(error) => memory_failure(core, error, width),
        }
    })
}
