use super::{TaskIncomingContext32, TaskTransitionFault32};
use crate::cpu::x86::{
    CodeTransfer, EnteredEvent32, SegmentCache, SegmentContext, SegmentRegister, State32,
};

/// a wire image of known incoming state; it is not a second live architectural owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskContextImage32 {
    pub state: State32,
    pub segments: SegmentContext,
    pub loaded: u8,
    pub ldtr: u16,
    pub ldt_cache: Option<crate::cpu::x86::SegmentDescriptorRecord>,
    pub task_trap: bool,
    pub fault: Option<TaskTransitionFault32>,
}

impl TaskIncomingContext32 {
    pub(crate) fn image(self) -> TaskContextImage32 {
        TaskContextImage32 {
            state: self.state(),
            segments: self.segment_image(),
            loaded: self.loaded_mask(),
            ldtr: self.ldtr(),
            ldt_cache: self.ldt_cache(),
            task_trap: self.task_trap(),
            fault: self.fault(),
        }
    }
    pub(crate) fn from_image(image: TaskContextImage32) -> Option<Self> {
        let cpl = (image.segments.state.selectors[1] & 3) as u8;
        if image.loaded & !0x3f != 0
            || image.state.eflags & 2 == 0
            || image.state.eflags & !0x254fd7 != 0
        {
            return None;
        }
        for index in 0..6 {
            let selector = image.segments.state.selectors[index];
            let cache = image.segments.caches[index];
            let base = image.segments.state.bases[index];
            if image.loaded & (1 << index) == 0 {
                if cache != SegmentCache::UNUSABLE || base != 0 {
                    return None;
                }
            } else if selector & !3 == 0 {
                if matches!(index, 1 | 2) || cache != SegmentCache::UNUSABLE || base != 0 {
                    return None;
                }
            } else if !cache.usable()
                || cache.bits() & (1 << 20) == 0
                || match index {
                    1 => {
                        !cache.code()
                            || if cache.conforming() {
                                cache.dpl() > cpl
                            } else {
                                cache.dpl() != cpl
                            }
                    }
                    2 => {
                        !cache.writable()
                            || cache.code()
                            || cache.dpl() != cpl
                            || selector & 3 != u16::from(cpl)
                    }
                    _ => {
                        !cache.readable()
                            || !cache.conforming() && cache.dpl() < cpl.max((selector & 3) as u8)
                    }
                }
            {
                return None;
            }
        }
        if let Some(record) = image.ldt_cache
            && (image.ldtr & !3 == 0
                || image.ldtr & 4 != 0
                || record.code_data()
                || record.descriptor_type() != 2
                || !record.present())
        {
            return None;
        }
        if let Some(fault) = image.fault {
            let code = match fault {
                TaskTransitionFault32::InvalidTss(code)
                | TaskTransitionFault32::NotPresent(code)
                | TaskTransitionFault32::StackSegment(code)
                | TaskTransitionFault32::GeneralProtection(code) => code,
            };
            if code > 0xfffc
                || code & 3 != 0
                || matches!(fault, TaskTransitionFault32::GeneralProtection(_)) && code != 0
                || matches!(
                    fault,
                    TaskTransitionFault32::NotPresent(_) | TaskTransitionFault32::StackSegment(_)
                ) && code == 0
            {
                return None;
            }
        }
        Some(Self {
            state: image.state,
            segments: image.segments,
            loaded: image.loaded,
            ldtr: image.ldtr,
            ldt_cache: image.ldt_cache,
            task_trap: image.task_trap,
            fault: image.fault,
        })
    }

    pub(crate) fn with_state(mut self, state: State32) -> Self {
        self.state = state;
        self
    }
    pub(crate) fn with_segments(mut self, segments: SegmentContext) -> Self {
        self.segments = segments;
        self
    }

    pub(crate) fn with_loaded_segment(
        mut self,
        register: SegmentRegister,
        selector: u16,
        base: u32,
        cache: SegmentCache,
    ) -> Self {
        let index = register.index();
        self.segments.state.selectors[index] = selector;
        self.load(register, base, cache);
        self
    }

    pub(crate) fn after_code_transfer(mut self, transfer: CodeTransfer) -> Self {
        transfer.apply(&mut self.segments);
        self.loaded |= 1 << SegmentRegister::Cs.index();
        self
    }

    /// the new handler replaces CS and stack position; unresolved data selectors remain raw/unusable.
    pub(crate) fn after_event(mut self, transfer: CodeTransfer, entered: EnteredEvent32) -> Self {
        let mut segments = self.segment_image();
        transfer.apply(&mut segments);
        self.segments = segments;
        self.loaded |= 1 << SegmentRegister::Cs.index();
        for index in [0, 3, 4, 5] {
            if self.segments.state.selectors[index] & !3 == 0 {
                self.loaded |= 1 << index;
            }
        }
        self.state.eip = entered.target;
        self.state.registers[4] = entered.esp;
        self.state.eflags = entered.flags;
        self.fault = None;
        self
    }
}
