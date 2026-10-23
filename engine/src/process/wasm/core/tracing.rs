use super::{CoreSession32, REGISTRY, Registry, cache::RegionRef, compile_status};
use crate::{
    cpu::core::{
        CompileOptions, compile_observed,
        trace::{self, TraceArena, TraceEvent},
    },
    memory::{AddressSpace, GuestAddress},
};
use foldhash::fast::RandomState;
use std::collections::HashSet;

pub(super) struct TraceController {
    pub arena: Box<TraceArena>,
    root: Option<u32>,
    memory_identity: u64,
    source_version: u64,
    owners: Vec<RegionRef>,
    ended: bool,
    manual: bool,
    attempt_epoch: (u64, u64),
    attempted_roots: HashSet<u32, RandomState>,
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
            attempt_epoch: (0, 0),
            attempted_roots: HashSet::with_hasher(RandomState::default()),
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
        self.ended = false;
        self.manual = manual;
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
        }
    }

    fn clear_capture(&mut self) {
        self.arena.deactivate();
        self.root = None;
        self.owners.clear();
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

impl CoreSession32 {
    pub(super) fn trace_selection(&mut self, memory: &AddressSpace, pc: u32) {
        let Some(code) = self.code.as_ref() else {
            return;
        };
        // Trace bodies deliberately omit baseline recording. Their execution
        // must not become an invisible gap between two confirmed entries.
        if code.trace_root().is_some() {
            self.tracing.end();
            return;
        }
        if self.tracing.root.is_some() {
            self.tracing.observe(code);
            return;
        }
        let epoch = (memory.identity(), memory.code_source_version());
        if self.tracing.attempt_epoch != epoch {
            self.tracing.attempt_epoch = epoch;
            self.tracing.attempted_roots.clear();
        }
        if self.tracing.attempted_roots.len() >= 4096
            || self.tracing.attempted_roots.contains(&pc)
            || !self.cache.trace_candidate(code, pc)
        {
            return;
        }
        self.tracing.attempted_roots.insert(pc);
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
        let Some(code) = core.code.as_ref().filter(|code| {
            code.trace_root().is_none() && code.supports_entry(root) && code.is_current(memory)
        }) else {
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
            && memory.code_source_version() == core.tracing.source_version;
        let result = if let Some(events) = events.filter(|_| valid) {
            let owners: Vec<_> = core.tracing.owners.iter().map(|owner| &**owner).collect();
            compile_observed(
                memory,
                GuestAddress(root),
                CompileOptions {
                    instructions: instructions as usize,
                    blocks: blocks as usize,
                    ..CompileOptions::default()
                },
                &events,
                &owners,
                core.tracing.source_version,
            )
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
