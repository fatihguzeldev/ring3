use super::TaskState32;
use crate::cpu::x86::{
    SegmentCache, SegmentContext, SegmentDescriptorRecord, SegmentRegister, SegmentState, State32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskTransitionFault32 {
    InvalidTss(u32),
    NotPresent(u32),
    StackSegment(u32),
    GeneralProtection(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskAuthorityRequirement32 {
    Sources,
    Controls,
    TaskFormat,
    AddressSpace,
    Ldt(u16),
    Descriptor(u16),
    Privilege(u8),
    Virtual8086,
    Flags(u32),
    SegmentMode(u16),
}

/// selectors and gprs are incoming even when later cache qualification fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskIncomingContext32 {
    pub(super) state: State32,
    pub(super) segments: SegmentContext,
    pub(super) loaded: u8,
    pub(super) ldtr: u16,
    pub(super) ldt_cache: Option<SegmentDescriptorRecord>,
    pub(super) task_trap: bool,
    pub(super) fault: Option<TaskTransitionFault32>,
}

impl TaskIncomingContext32 {
    pub(crate) fn new(incoming: TaskState32) -> Self {
        Self {
            state: State32 {
                registers: incoming.general,
                eip: incoming.eip,
                eflags: (incoming.flags & 0x003f_7fd7) | 2,
            },
            segments: SegmentContext {
                state: SegmentState {
                    selectors: incoming.selectors,
                    bases: [0; 6],
                },
                caches: [SegmentCache::UNUSABLE; 6],
            },
            loaded: 0,
            ldtr: incoming.ldtr,
            ldt_cache: None,
            task_trap: incoming.task_trap,
            fault: None,
        }
    }
    pub const fn state(self) -> State32 {
        self.state
    }
    pub const fn selectors(self) -> [u16; 6] {
        self.segments.state.selectors
    }
    pub const fn segment_image(self) -> SegmentContext {
        self.segments
    }
    pub const fn loaded_mask(self) -> u8 {
        self.loaded
    }
    pub const fn ldtr(self) -> u16 {
        self.ldtr
    }
    pub const fn ldt_cache(self) -> Option<SegmentDescriptorRecord> {
        self.ldt_cache
    }
    pub(crate) fn load_ldtr(&mut self, cache: Option<SegmentDescriptorRecord>) {
        self.ldt_cache = cache;
    }
    pub const fn task_trap(self) -> bool {
        self.task_trap
    }
    pub const fn fault(self) -> Option<TaskTransitionFault32> {
        self.fault
    }
    pub const fn current_privilege(self) -> u8 {
        (self.segments.state.selectors[1] & 3) as u8
    }
    pub fn validated_segments(self) -> Option<SegmentContext> {
        (self.loaded == 0x3f && self.segments.is_valid()).then_some(self.segments)
    }
    pub(crate) fn load(&mut self, register: SegmentRegister, base: u32, cache: SegmentCache) {
        let index = register.index();
        self.segments.state.bases[index] = base;
        self.segments.caches[index] = cache;
        self.loaded |= 1 << index;
    }
    pub(crate) fn fail(&mut self, fault: TaskTransitionFault32) {
        self.fault = Some(fault);
    }
}
