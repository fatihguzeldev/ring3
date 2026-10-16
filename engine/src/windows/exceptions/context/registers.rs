use super::*;
use crate::cpu::x86::SegmentRegister;

const INTEGER: [(usize, usize); 6] = [(156, 7), (160, 6), (164, 3), (168, 2), (172, 1), (176, 0)];
const DATA: [(usize, SegmentRegister); 4] = [
    (140, SegmentRegister::Gs),
    (144, SegmentRegister::Fs),
    (148, SegmentRegister::Es),
    (152, SegmentRegister::Ds),
];
const CONTROL: [(usize, SegmentRegister); 2] =
    [(188, SegmentRegister::Cs), (200, SegmentRegister::Ss)];
const USER_FLAGS: u32 = 0x0025_0dd5;

impl Win32Context32 {
    pub fn capture(flags: u32, state: &ContextState32) -> Result<Self, ContextError32> {
        validate_flags(flags)?;
        if !state.segments.is_valid() {
            return Err(ContextError32::InvalidSegment);
        }
        let mut result = Self {
            bytes: [0; CONTEXT_SIZE32],
        };
        result.write(0, flags);
        if result.selected(0x10) {
            for (index, value) in state.debug.iter().enumerate() {
                result.write(4 + index * 4, *value);
            }
        }
        if result.selected(2) {
            for (offset, index) in INTEGER {
                result.write(offset, state.state.registers[index]);
            }
        }
        if result.selected(4) {
            for (offset, segment) in DATA {
                result.write(
                    offset,
                    u32::from(state.segments.state.selectors[segment.index()]),
                );
            }
        }
        if result.selected(1) {
            for (offset, segment) in CONTROL {
                result.write(
                    offset,
                    u32::from(state.segments.state.selectors[segment.index()]),
                );
            }
            for (offset, value) in [
                (180, state.state.registers[5]),
                (184, state.state.eip),
                (192, state.state.eflags),
                (196, state.state.registers[4]),
            ] {
                result.write(offset, value);
            }
        }
        if result.selected(8) {
            result.save_floating(&state.x87);
        }
        if result.selected(0x20) {
            result.save_extended(&state.x87, &state.simd);
            result.extended()?;
        }
        Ok(result)
    }
    pub fn requested_eip(&self, base: &ContextState32) -> u32 {
        if self.selected(1) { self.word(184) } else { base.state.eip }
    }
    pub fn requested_debug_registers(&self, base: &ContextState32) -> [u32; 6] {
        if self.selected(0x10) { std::array::from_fn(|index| self.word(4 + index * 4)) } else { base.debug }
    }
    pub fn requested_selectors(&self, base: &ContextState32) -> [u16; 6] {
        let mut selectors = base.segments.state.selectors;
        if self.selected(4) {
            for (offset, segment) in DATA {
                selectors[segment.index()] = self.word(offset) as u16;
            }
        }
        if self.selected(1) {
            for (offset, segment) in CONTROL {
                selectors[segment.index()] = self.word(offset) as u16;
            }
        }
        selectors
    }
    pub fn apply(
        &self,
        base: &ContextState32,
        resolved_segments: SegmentContext,
    ) -> Result<ContextState32, ContextError32> {
        validate_flags(self.flags())?;
        if !resolved_segments.is_valid()
            || resolved_segments.state.selectors != self.requested_selectors(base)
        {
            return Err(ContextError32::InvalidSegment);
        }
        let mut state = *base;
        state.debug = self.requested_debug_registers(base);
        if self.selected(2) {
            for (offset, index) in INTEGER {
                state.state.registers[index] = self.word(offset);
            }
        }
        for (selected, segments) in [
            (self.selected(4), DATA.as_slice()),
            (self.selected(1), CONTROL.as_slice()),
        ] {
            if selected {
                for &(_, segment) in segments {
                    let index = segment.index();
                    state.segments.state.selectors[index] =
                        resolved_segments.state.selectors[index];
                    state.segments.state.bases[index] = resolved_segments.state.bases[index];
                    state.segments.caches[index] = resolved_segments.caches[index];
                }
            }
        }
        if self.selected(1) {
            state.state.registers[5] = self.word(180);
            state.state.eip = self.word(184);
            state.state.registers[4] = self.word(196);
            state.state.eflags =
                (base.state.eflags & !USER_FLAGS) | (self.word(192) & USER_FLAGS) | 2;
        }
        if self.selected(8) {
            state.x87 = self.floating();
        }
        if self.selected(0x20) {
            (state.x87, state.simd) = self.extended()?;
        }
        Ok(state)
    }
}
