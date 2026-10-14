use crate::{
    abi::{
        arena::HELPER_OFFSET,
        cpu_helper::CpuMemoryError,
        interrupt::{InterruptHelperError, encode_interrupt_result},
        memory_helper::HELPER_SIZE,
        x86::USER_FLAGS_MASK_V3,
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::{
        CodeTransfer, CodeTransferError, SegmentAccess, SegmentContext, SegmentRegister,
        StackPointerSize, USER_MODE_ENVIRONMENT,
    },
    process::{EngineInstance, HostError, StoreCompletion},
};

struct PreparedInterrupt {
    segments: [u8; SEGMENTS_SIZE],
    bases: [u8; SEGMENT_BASES_RECORD_SIZE],
    packet: [u8; HELPER_SIZE],
    selector: u16,
}

impl EngineInstance {
    pub fn interrupt_gate(
        &mut self,
        vector: u32,
        return_eip: u32,
        esp: u32,
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.interrupt_gate_bound(None, vector, return_eip, esp, flags)
    }

    pub fn interrupt_gate_resident(
        &mut self,
        key: u64,
        id: u64,
        vector: u32,
        return_eip: u32,
        esp: u32,
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.interrupt_gate_bound(Some(id), vector, return_eip, esp, flags)
    }

    fn interrupt_gate_bound(
        &mut self,
        resident: Option<u64>,
        vector: u32,
        return_eip: u32,
        esp: u32,
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        let vector = u8::try_from(vector).map_err(|_| HostError::InvalidRequest)?;
        if flags & 2 == 0 || flags & !USER_FLAGS_MASK_V3 != 0 {
            return Err(HostError::InvalidRequest);
        }
        let Some(table) = self.interrupt_descriptors.as_ref() else {
            return self.interrupt_result(vector, 0, Err(InterruptHelperError::Unsupported));
        };
        let cpl = USER_MODE_ENVIRONMENT.current_privilege();
        let gate = match table.resolve_gate(vector, cpl) {
            Ok(gate) => gate,
            Err(error) => return self.interrupt_transfer_error(vector, 0, error),
        };
        let frame_info = gate.frame_info();
        let destination = match gate.destination(&self.segment_descriptors, cpl) {
            Ok(destination) => destination,
            Err(error) => return self.interrupt_transfer_error(vector, frame_info, error),
        };
        // this cut admits only the explicitly supplied fixed AM0 environment.
        if USER_MODE_ENVIRONMENT.alignment_mask_enabled() {
            return self.interrupt_result(vector, 0, Err(InterruptHelperError::Unsupported));
        }
        let context = self.segment_context()?;
        let slot = gate.width().slot_bytes();
        let pointer =
            StackPointerSize::new(context.caches[SegmentRegister::Ss.index()].default32());
        let offsets =
            [1, 2, 3].map(|index| pointer.offset(pointer.advance(esp, -((index * slot) as i32))));
        // all cached logical slots precede target-limit and physical page checks.
        for offset in offsets {
            if let Err(error) = super::super::segments::check_cached_span(
                &context,
                SegmentRegister::Ss,
                offset,
                slot,
                SegmentAccess::Write,
            ) {
                return self.interrupt_result(
                    vector,
                    frame_info,
                    Err(InterruptHelperError::Cpu(error)),
                );
            }
        }
        let transfer = match destination.target(gate.offset(), gate.width()) {
            Ok(transfer) => transfer,
            Err(error) => return self.interrupt_transfer_error(vector, frame_info, error),
        };
        let prepared = self.prepare_interrupt(vector, frame_info, context, transfer)?;
        let values = [
            flags & !0x10000,
            u32::from(context.state.selectors[SegmentRegister::Cs.index()]),
            gate.width().offset(return_eip),
        ];
        let ss_base = context.state.bases[SegmentRegister::Ss.index()];
        let addresses = offsets.map(|offset| ss_base.wrapping_add(offset));
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        if let Err(error) = memory.cpu_write_stack(&addresses, &values, slot as usize) {
            return self.interrupt_result(
                vector,
                frame_info,
                Err(InterruptHelperError::Cpu(CpuMemoryError::Memory(error))),
            );
        }
        let changed = version != memory.code_validation_version();
        // no fallible encoding or host reentry may separate frame publication from CS commit.
        self.publish_segment_context(&prepared.segments, &prepared.bases);
        self.segment_descriptors.commit_accessed(prepared.selector);
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&prepared.packet);
        self.cpu_store_currentness(changed, resident)
    }

    fn prepare_interrupt(
        &self,
        vector: u8,
        frame_info: u32,
        mut context: SegmentContext,
        transfer: CodeTransfer,
    ) -> Result<PreparedInterrupt, HostError> {
        transfer.apply(&mut context);
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases)
            .map_err(|_| HostError::Infrastructure)?;
        let mut packet = [0; HELPER_SIZE];
        encode_interrupt_result(vector, frame_info, transfer.offset, Ok(()), &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(PreparedInterrupt {
            segments,
            bases,
            packet,
            selector: transfer.selector(),
        })
    }

    fn interrupt_transfer_error(
        &mut self,
        vector: u8,
        frame_info: u32,
        error: CodeTransferError,
    ) -> Result<StoreCompletion, HostError> {
        let error = match error {
            CodeTransferError::GeneralProtection(code) => {
                InterruptHelperError::Cpu(CpuMemoryError::GeneralProtection(code))
            }
            CodeTransferError::NotPresent(code) => {
                InterruptHelperError::Cpu(CpuMemoryError::SegmentNotPresent(code))
            }
            CodeTransferError::UnsupportedProfile => InterruptHelperError::Unsupported,
        };
        let frame_info = if matches!(error, InterruptHelperError::Unsupported) {
            0
        } else {
            frame_info
        };
        self.interrupt_result(vector, frame_info, Err(error))
    }

    fn interrupt_result(
        &mut self,
        vector: u8,
        frame_info: u32,
        result: Result<(), InterruptHelperError>,
    ) -> Result<StoreCompletion, HostError> {
        let mut packet = [0; HELPER_SIZE];
        encode_interrupt_result(vector, frame_info, 0, result, &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        Ok(StoreCompletion::Complete)
    }
}

#[cfg(test)]
mod tests;
