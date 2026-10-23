use super::{CoreSession32, REGISTRY, Registry, cache::RegionRef, compile_status};
use crate::{
    cpu::core::{
        CompileOptions,
        cold::ColdObserver,
        compile_observed_with_cold, compile_observed_with_growth,
        trace::{self, CompletedColdObservation, PendingColdObservation, TraceArena, TraceEvent},
    },
    memory::{AddressSpace, GuestAddress},
};

pub(super) struct TraceController {
    pub arena: Box<TraceArena>,
    root: Option<u32>,
    memory_identity: u64,
    source_version: u64,
    owners: Vec<RegionRef>,
    ended: bool,
    manual: bool,
    cold: Vec<CompletedColdObservation>,
    pending: Option<PendingColdObservation>,
}

impl Default for TraceController {
    fn default() -> Self {
        Self {
            arena: Box::new(TraceArena::new()),
            root: None,
            memory_identity: 0,
            source_version: 0,
            owners: Vec::new(),
            ended: false,
            manual: false,
            cold: Vec::new(),
            pending: None,
        }
    }
}

impl TraceController {
    fn start(&mut self, memory: &AddressSpace, code: RegionRef, root: u32, manual: bool) {
        self.arena.begin();
        self.root = Some(root);
        self.memory_identity = memory.identity();
        self.source_version = memory.code_source_version();
        self.owners.clear();
        self.owners.push(code);
        self.cold.clear();
        self.pending = None;
        self.ended = false;
        self.manual = manual;
    }

    fn start_cold(&mut self, memory: &AddressSpace, root: u32) {
        self.arena.begin();
        self.root = Some(root);
        self.memory_identity = memory.identity();
        self.source_version = memory.code_source_version();
        self.owners.clear();
        self.cold.clear();
        self.pending = None;
        self.ended = false;
        self.manual = false;
    }

    pub(super) fn cold_boundary(&mut self) {
        if self.manual {
            self.end();
        }
    }

    pub(super) fn observe(&mut self, code: &RegionRef) {
        if self.arena.active != 1
            || self
                .owners
                .iter()
                .any(|owner| owner.identity() == code.identity())
        {
            return;
        }
        if self.owners.len() == trace::CAPACITY {
            self.arena.overflow = 1;
            self.arena.deactivate();
        } else {
            self.owners.push(code.clone());
        }
    }

    pub(super) fn numeric(&mut self, code: &RegionRef, pc: u32, next: u32) {
        if self.arena.active != 1 {
            return;
        }
        self.observe(code);
        self.arena.push(TraceEvent {
            kind: trace::NUMERIC,
            pc,
            end: next,
            offset: 0,
            owner: code.identity(),
        });
    }

    pub(super) fn end(&mut self) {
        if self.root.is_some() {
            self.arena.deactivate();
            self.ended = true;
            self.pending = None;
        }
    }

    fn clear_capture(&mut self) {
        self.arena.deactivate();
        self.root = None;
        self.owners.clear();
        self.cold.clear();
        self.pending = None;
        self.ended = false;
        self.manual = false;
    }

    pub(super) fn bypass(&mut self, retired: u32) -> u64 {
        if self.arena.bypass_pending == 1 && self.arena.bypass_retired != retired {
            self.arena.bypass_pending = 0;
            self.arena.bypass_id = 0;
        }
        if self.arena.bypass_pending == 1 {
            self.arena.bypass_id
        } else {
            0
        }
    }
}

impl ColdObserver for TraceController {
    fn bypass_token(&self) -> Option<(u64, u32)> {
        (self.arena.bypass_pending == 1)
            .then_some((self.arena.bypass_id, self.arena.bypass_retired))
    }
    fn exact_cold(&self) -> bool {
        true
    }
    fn begin_instruction(
        &mut self,
        memory: &AddressSpace,
        instruction: &crate::cpu::core::NativeInstruction,
        hot: bool,
    ) -> bool {
        if self.root.is_none() && hot {
            self.start_cold(memory, instruction.pc);
        }
        if self.arena.active == 1 {
            self.pending = PendingColdObservation::capture(memory, instruction);
            if self.pending.is_none() {
                self.end();
            }
        }
        true
    }

    fn complete_instruction(&mut self, memory: &AddressSpace, actual_post_pc: u32) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(completed) = pending.complete(memory, actual_post_pc) else {
            self.end();
            return;
        };
        let Some(event) = completed.event(self.cold.len()) else {
            self.end();
            return;
        };
        if self.arena.push(event) {
            self.cold.push(completed);
        }
        if self.arena.active != 1 {
            self.end();
        }
    }

    fn abandon_instruction(&mut self) {
        self.end();
    }
}

impl CoreSession32 {
    pub(super) fn trace_selection(&mut self, memory: &AddressSpace, pc: u32) {
        let Some(code) = self.code.as_ref() else {
            return;
        };
        if self.tracing.root.is_some() {
            self.tracing.observe(code);
            return;
        }
        if !self.cache.trace_candidate(code, pc) {
            return;
        }
        self.tracing.start(memory, code.clone(), pc, false);
    }

    pub(super) fn refresh_trace_bypass(&mut self, retired: u32) {
        let identity = self.tracing.bypass(retired);
        self.cache.bypass_trace(identity);
    }
}

pub(crate) fn core_trace_pointer() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 0;
        };
        let Some(core) = registry.core.as_mut().filter(|core| !core.active) else {
            return 0;
        };
        (&mut *core.tracing.arena as *mut TraceArena) as usize as u32
    })
}

pub(crate) fn core_trace_status_ptr() -> u32 {
    core_trace_pointer()
}

pub(crate) fn core_trace_start(root: u32) -> u32 {
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
        if core.tracing.root.is_some() {
            return 12;
        }
        let Some(memory) = instance
            .as_ref()
            .and_then(|instance| instance.memory.as_ref())
        else {
            return 5;
        };
        let Some(code) = core
            .code
            .as_ref()
            .filter(|code| code.supports_entry(root) && code.is_current(memory))
        else {
            return 4;
        };
        core.tracing.start(memory, code.clone(), root, true);
        0
    })
}

// Auto compilation is performed outside generated execution. The SDK installs
// the prepared immutable module before another chain can select its table slot.
pub(crate) fn core_trace_ready() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 0;
        };
        u32::from(registry.core.as_ref().is_some_and(|core| {
            !core.active
                && core.tracing.root.is_some()
                && core.tracing.ended
                && !core.tracing.manual
        }))
    })
}

pub(crate) fn core_trace_root() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .core
                    .as_ref()
                    .filter(|core| !core.active)
                    .and_then(|core| core.tracing.root)
            })
            .unwrap_or(0)
    })
}

pub(crate) fn core_trace_finish(root: u32, instructions: u32, blocks: u32) -> u32 {
    if instructions == 0 || instructions > 16384 || blocks == 0 || blocks > 1024 {
        return 7;
    }
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
        if core.tracing.root != Some(root) || !core.tracing.ended {
            return 7;
        }
        let Some(memory) = instance
            .as_ref()
            .and_then(|instance| instance.memory.as_ref())
        else {
            return 5;
        };
        let events = core.tracing.arena.snapshot().map(<[TraceEvent]>::to_vec);
        let valid = memory.identity() == core.tracing.memory_identity
            && (!core.tracing.manual
                || memory.code_source_version() == core.tracing.source_version);
        let result = if let Some(events) = events.filter(|_| valid) {
            let owners: Vec<_> = core.tracing.owners.iter().map(|owner| &**owner).collect();
            let options = CompileOptions {
                instructions: instructions as usize,
                blocks: blocks as usize,
                ..CompileOptions::default()
            };
            if !core.tracing.manual {
                let previous = core.cache.observed_owner(memory, root).or_else(|| {
                    // A cold prefix may return to one retained family. Its held
                    // immutable history remains the anchor even when this new
                    // capture starts at a previously unowned instruction.
                    let mut families = core
                        .tracing
                        .owners
                        .iter()
                        .filter(|owner| owner.is_observed_family() && owner.is_current(memory));
                    let family = families.next()?;
                    families.next().is_none().then(|| family.clone())
                });
                compile_observed_with_cold(
                    memory,
                    GuestAddress(root),
                    options,
                    &events,
                    &owners,
                    memory.code_source_version(),
                    &core.tracing.cold,
                    previous.as_ref().map(|owner| &**owner),
                )
            } else {
                compile_observed_with_growth(
                    memory,
                    GuestAddress(root),
                    options,
                    &events,
                    &owners,
                    core.tracing.source_version,
                    core.cache.trace_score(memory, root),
                )
            }
        } else {
            Ok(None)
        };
        core.tracing.clear_capture();
        let code = match result {
            Ok(Some(code)) => code,
            Ok(None) => return 18,
            Err(error) => return compile_status(error),
        };
        let code = match core.cache.admit(memory, code) {
            Ok(code) => code,
            Err(error) => return compile_status(error),
        };
        let identity = code.identity();
        core.code = Some(code);
        core.context.bytes_mut()[120..128].copy_from_slice(&identity.to_le_bytes());
        core.context.set_word(112, 0);
        0
    })
}
