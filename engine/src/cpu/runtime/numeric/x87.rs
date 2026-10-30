use super::*;

impl NumericState {
    pub(super) fn x87(
        &mut self,
        memory: &mut AddressSpace,
        scalar: &mut State32,
        segments: &SegmentContext,
        operation: X87Operation,
        address: Option<EffectiveAddress>,
        waiting: bool,
        opcode: u16,
    ) -> Result<(), NumericError> {
        if waiting && self.x87.status & !self.x87.control & 63 != 0 {
            self.x87.status |= 0x8080;
            return Err(NumericError::Cpu {
                vector: 16,
                detail: u32::from(self.x87.status),
            });
        }
        let read = operation.read_size();
        let write = operation.write_size();
        let width = read.max(write);
        let mut buffer = [0; 108];
        let mut linear = 0;
        let mut data_offset = 0;
        if width != 0 {
            let address = address.ok_or(NumericError::Infrastructure)?;
            data_offset = offset(address, scalar);
            linear = span(
                segments,
                address.resolved_segment(),
                data_offset,
                width,
                if write != 0 {
                    SegmentAccess::Write
                } else {
                    SegmentAccess::Read
                },
            )?;
            let mut port = memory::Port(memory);
            if read != 0 {
                port.read(GuestAddress(linear), &mut buffer[..read])
                    .map_err(|error| NumericError::memory(error, read))?;
            }
            if write != 0 {
                port.check(GuestAddress(linear), write, PagingAccess32::Write)
                    .map_err(|error| NumericError::memory(error, write))?;
            }
        }
        let before = (write != 0).then_some(self.x87);
        let metadata = (
            self.x87.instruction_pointer,
            self.x87.data_pointer,
            self.x87.opcode,
            self.x87.code_selector,
            self.x87.data_selector,
        );
        if operation.is_computation() {
            self.x87.instruction_pointer = scalar.eip;
            self.x87.opcode = opcode;
            self.x87.code_selector = segments.state.selectors[SegmentRegister::Cs.index()];
            if let Some(address) = address.filter(|_| width != 0) {
                self.x87.data_pointer = data_offset;
                self.x87.data_selector =
                    segments.state.selectors[address.resolved_segment().index()];
            }
        }
        let result = if write == 0 {
            self.x87.evaluate_in_place(
                operation,
                &buffer[..read],
                scalar.eflags,
                None,
                &mut self.workspace,
            )
        } else {
            self.x87.evaluate_in_place(
                operation,
                &[],
                scalar.eflags,
                Some(&mut buffer[..write]),
                &mut self.workspace,
            )
        };
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                if let Some(before) = before {
                    self.x87 = before;
                }
                (
                    self.x87.instruction_pointer,
                    self.x87.data_pointer,
                    self.x87.opcode,
                    self.x87.code_selector,
                    self.x87.data_selector,
                ) = metadata;
                return Err(NumericError::Infrastructure);
            }
        };
        if result.memory_len != 0 {
            if let Err(error) =
                memory::Port(memory).write(GuestAddress(linear), &buffer[..result.memory_len])
            {
                self.x87 = before.ok_or(NumericError::Infrastructure)?;
                return Err(NumericError::memory(error, write));
            }
        }
        if let Some(flags) = result.eflags {
            scalar.eflags = flags | 2;
        }
        if operation == X87Operation::StatusToAx {
            scalar.registers[0] = (scalar.registers[0] & 0xffff_0000) | u32::from(self.x87.status);
        }
        Ok(())
    }
}
