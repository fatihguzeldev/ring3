use super::*;
use crate::cpu::x86::simd::{restore_extended, save_extended};

impl Win32Context32 {
    pub(super) fn save_floating(&mut self, state: &X87State) {
        for (offset, value) in [
            (28, u32::from(state.control)),
            (32, u32::from(state.status)),
            (36, u32::from(state.tag)),
            (40, state.instruction_pointer),
            (
                44,
                u32::from(state.code_selector) | (u32::from(state.opcode & 0x7ff) << 16),
            ),
            (48, state.data_pointer),
            (52, u32::from(state.data_selector)),
        ] {
            self.write(offset, value);
        }
        let top = usize::from((state.status >> 11) & 7);
        for logical in 0..8 {
            self.bytes[56 + logical * 10..66 + logical * 10]
                .copy_from_slice(&state.registers[(top + logical) & 7]);
        }
        self.write(136, 0);
    }
    pub(super) fn floating(&self) -> X87State {
        let status = self.word(32) as u16;
        let top = usize::from((status >> 11) & 7);
        X87State {
            control: self.word(28) as u16,
            status,
            tag: self.word(36) as u16,
            opcode: (self.word(44) >> 16) as u16 & 0x7ff,
            instruction_pointer: self.word(40),
            code_selector: self.word(44) as u16,
            data_pointer: self.word(48),
            data_selector: self.word(52) as u16,
            registers: std::array::from_fn(|physical| {
                let logical = (physical + 8 - top) & 7;
                self.bytes[56 + logical * 10..66 + logical * 10]
                    .try_into()
                    .unwrap()
            }),
        }
    }
    pub(super) fn save_extended(&mut self, x87: &X87State, simd: &SimdState) {
        save_extended(x87, simd, (&mut self.bytes[204..716]).try_into().unwrap());
    }
    pub(super) fn extended(&self) -> Result<(X87State, SimdState), ContextError32> {
        restore_extended((&self.bytes[204..716]).try_into().unwrap())
            .map_err(|_| ContextError32::InvalidFloatingPoint)
    }
}
