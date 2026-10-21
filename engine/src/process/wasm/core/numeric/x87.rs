use super::*;
use crate::cpu::x86::{
    CpuMemoryPort32, CpuReadPort32, PagingAccess32, Register32, SegmentAccess, SegmentRegister,
    X87Error, X87Operation,
    ir::{AddressSize, EffectiveAddress},
};

struct Metadata {
    instruction_pointer: u32,
    data_pointer: u32,
    opcode: u16,
    code_selector: u16,
    data_selector: u16,
}

struct OutputState {
    control: u16,
    status: u16,
    tag: u16,
}

impl OutputState {
    fn capture(state: &X87State) -> Self {
        Self {
            control: state.control,
            status: state.status,
            tag: state.tag,
        }
    }

    fn restore(self, state: &mut X87State) {
        state.control = self.control;
        state.status = self.status;
        state.tag = self.tag;
    }
}

impl Metadata {
    fn capture(state: &X87State) -> Self {
        Self {
            instruction_pointer: state.instruction_pointer,
            data_pointer: state.data_pointer,
            opcode: state.opcode,
            code_selector: state.code_selector,
            data_selector: state.data_selector,
        }
    }

    fn restore(self, state: &mut X87State) {
        state.instruction_pointer = self.instruction_pointer;
        state.data_pointer = self.data_pointer;
        state.opcode = self.opcode;
        state.code_selector = self.code_selector;
        state.data_selector = self.data_selector;
    }
}

fn word(context: &Context32, offset: usize) -> u32 {
    u32::from_le_bytes(context.bytes()[offset..offset + 4].try_into().unwrap())
}

fn offset(address: EffectiveAddress, context: &Context32) -> u32 {
    let register = |register: Register32| word(context, register.index() * 4);
    let value = address
        .displacement
        .wrapping_add(address.base.map_or(0, register))
        .wrapping_add(address.index.map_or(0, |index| {
            register(index).wrapping_mul(u32::from(address.scale))
        }));
    match address.address_size {
        AddressSize::Bits16 => value & 0xffff,
        AddressSize::Bits32 => value,
    }
}

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
        debug_assert!(read == 0 || write == 0);
        let mut buffer = (width != 0).then(|| [0; 108]);
        let mut linear = 0;
        let mut data_offset = 0;
        if width != 0 {
            let address = address.ok_or(Failure::Infrastructure)?;
            data_offset = offset(address, context);
            if read != 0 {
                linear = self.span(
                    address.resolved_segment(),
                    data_offset,
                    read,
                    SegmentAccess::Read,
                )?;
            }
            if write != 0 {
                linear = self.span(
                    address.resolved_segment(),
                    data_offset,
                    write,
                    SegmentAccess::Write,
                )?;
            }
            let memory = instance.memory.as_mut().ok_or(Failure::Infrastructure)?;
            let mut port = memory::Port(memory);
            if read != 0 {
                port.read(GuestAddress(linear), &mut buffer.as_mut().unwrap()[..read])
                    .map_err(|e| Failure::paging(e, read))?;
            }
            if write != 0 {
                port.check(GuestAddress(linear), write, PagingAccess32::Write)
                    .map_err(|e| Failure::paging(e, write))?;
            }
        }
        debug_assert!(
            write == 0
                || matches!(
                    operation,
                    X87Operation::Store { .. }
                        | X87Operation::StoreControl
                        | X87Operation::StoreStatus
                        | X87Operation::StoreEnvironment { .. }
                        | X87Operation::Save { .. }
                )
        );
        // memory-output operations preserve all register payloads, including save.
        let before = (write != 0).then(|| OutputState::capture(&self.x87));
        let metadata = Metadata::capture(&self.x87);
        if operation.is_computation() {
            self.x87.instruction_pointer = instruction.pc;
            self.x87.opcode = opcode;
            self.x87.code_selector = self.segments.state.selectors[SegmentRegister::Cs.index()];
            if let Some(address) = address.filter(|_| width != 0) {
                self.x87.data_pointer = data_offset;
                self.x87.data_selector =
                    self.segments.state.selectors[address.resolved_segment().index()];
            }
        }
        let flags = word(context, 36);
        let result = if write == 0 {
            self.x87.evaluate_in_place(
                operation,
                buffer.as_ref().map_or(&[][..], |buffer| &buffer[..read]),
                flags,
                None,
                &mut instance.numerical_workspace,
            )
        } else {
            self.x87.evaluate_in_place(
                operation,
                &[],
                flags,
                buffer.as_mut(),
                &mut instance.numerical_workspace,
            )
        };
        let effects = match result {
            Ok(effects) => effects,
            Err(
                X87Error::InvalidInputLength
                | X87Error::InvalidOperand
                | X87Error::NumericalFailure,
            ) => {
                if let Some(before) = before {
                    before.restore(&mut self.x87);
                }
                // evaluation already restores its state, but starts after our metadata update.
                metadata.restore(&mut self.x87);
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
                        .write(
                            GuestAddress(linear),
                            &buffer.as_ref().unwrap()[..effects.memory_len],
                        )
                        .map_err(|e| Failure::paging(e, write))
                });
            if let Err(error) = result {
                before
                    .expect("x87 memory output has an outer rollback snapshot")
                    .restore(&mut self.x87);
                metadata.restore(&mut self.x87);
                return Err(error);
            }
        }
        if let Some(flags) = effects.eflags {
            context.set_word(36, flags | 2);
        }
        if matches!(operation, X87Operation::StatusToAx) {
            context.set_word(
                0,
                (word(context, 0) & 0xffff_0000) | u32::from(self.x87.status),
            );
        }
        Ok(())
    }
}
