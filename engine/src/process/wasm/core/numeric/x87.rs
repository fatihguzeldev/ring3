use super::*;
use crate::cpu::x86::{
    CpuMemoryPort32, CpuReadPort32, PagingAccess32, Register32, SegmentAccess, SegmentRegister,
    X87Error, X87Operation, ir::EffectiveAddress,
};

impl NumericBank {
    pub(super) fn x87(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        instruction: &NativeInstruction,
        operation: X87Operation,
        address: Option<EffectiveAddress>,
        waiting: bool,
        opcode: u16,
    ) -> NumericResult<()> {
        if waiting && self.x87.status & !self.x87.control & 63 != 0 {
            self.x87.status |= 0x8080;
            return Err(Failure::Cpu(16, u32::from(self.x87.status)));
        }
        let read = operation.read_size();
        let write = operation.write_size();
        let width = read.max(write);
        let state = context.state();
        let mut input = [0; 108];
        let mut output = [0; 108];
        let mut linear = 0;
        if width != 0 {
            let address = address.ok_or(Failure::Infrastructure)?;
            if read != 0 {
                linear = self.operand(address, &state, read, SegmentAccess::Read)?;
            }
            if write != 0 {
                linear = self.operand(address, &state, write, SegmentAccess::Write)?;
            }
            let memory = instance.memory.as_mut().ok_or(Failure::Infrastructure)?;
            let mut port = memory::Port(memory);
            if read != 0 {
                port.read(GuestAddress(linear), &mut input[..read])
                    .map_err(|e| Failure::paging(e, read))?;
            }
            if write != 0 {
                port.check(GuestAddress(linear), write, PagingAccess32::Write)
                    .map_err(|e| Failure::paging(e, write))?;
            }
        }
        let before = self.x87;
        if operation.is_computation() {
            self.x87.instruction_pointer = instruction.pc;
            self.x87.opcode = opcode;
            self.x87.code_selector = self.segments.state.selectors[SegmentRegister::Cs.index()];
            if let Some(address) = address.filter(|_| width != 0) {
                self.x87.data_pointer = memory::offset(address, &state);
                self.x87.data_selector =
                    self.segments.state.selectors[address.resolved_segment().index()];
            }
        }
        let effects = match self.x87.evaluate_in_place(
            operation,
            &input[..read],
            state.eflags,
            (write != 0).then_some(&mut output),
            &mut instance.numerical_workspace,
        ) {
            Ok(effects) => effects,
            Err(
                X87Error::InvalidInputLength
                | X87Error::InvalidOperand
                | X87Error::NumericalFailure,
            ) => {
                self.x87 = before;
                return Err(Failure::Infrastructure);
            }
        };
        if effects.memory_len != 0 {
            let result = instance
                .memory
                .as_mut()
                .ok_or(Failure::Infrastructure)
                .and_then(|m| {
                    memory::Port(m)
                        .write(GuestAddress(linear), &output[..effects.memory_len])
                        .map_err(|e| Failure::paging(e, write))
                });
            if let Err(error) = result {
                self.x87 = before;
                return Err(error);
            }
        }
        let mut state = state;
        if let Some(flags) = effects.eflags {
            state.eflags = flags;
        }
        if operation == X87Operation::StatusToAx {
            state.registers[Register32::Eax.index()] = (state.registers[Register32::Eax.index()]
                & 0xffff_0000)
                | u32::from(self.x87.status);
        }
        context.set_state(state);
        Ok(())
    }
}
