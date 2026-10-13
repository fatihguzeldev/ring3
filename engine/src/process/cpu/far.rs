use crate::{
    abi::{
        arena::HELPER_OFFSET,
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::{
        CodeTransfer, CodeTransferError, FarWidth, SegmentContext, SegmentRegister,
        USER_MODE_ENVIRONMENT,
    },
    process::{EngineInstance, HostError},
};

struct PreparedTransfer {
    segments: [u8; SEGMENTS_SIZE],
    bases: [u8; SEGMENT_BASES_RECORD_SIZE],
    packet: [u8; HELPER_SIZE],
    selector: u16,
}

fn width(word: u32) -> Result<FarWidth, HostError> {
    match word {
        0 => Ok(FarWidth::Dword),
        1 => Ok(FarWidth::Word),
        _ => Err(HostError::InvalidRequest),
    }
}

impl EngineInstance {
    pub fn far_jump(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.far_jump_bound(offset, selector, word)
    }

    pub fn far_jump_resident(
        &mut self,
        key: u64,
        id: u64,
        offset: u32,
        selector: u32,
        word: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.far_jump_bound(offset, selector, word)
    }

    fn far_jump_bound(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        let width = width(word)?;
        let selector = u16::try_from(selector).map_err(|_| HostError::InvalidRequest)?;
        let context = self.segment_context()?;
        let result = self.segment_descriptors.resolve_far_jump(
            selector,
            offset,
            width,
            USER_MODE_ENVIRONMENT.current_privilege(),
        );
        match result {
            Ok(transfer) => {
                let prepared = self.prepare_code_transfer(context, transfer)?;
                self.commit_code_transfer(prepared);
                Ok(())
            }
            Err(error) => self.code_transfer_fault(error),
        }
    }

    fn prepare_code_transfer(
        &self,
        mut context: SegmentContext,
        transfer: CodeTransfer,
    ) -> Result<PreparedTransfer, HostError> {
        transfer.apply(&mut context);
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases)
            .map_err(|_| HostError::Infrastructure)?;
        let mut packet = [0; HELPER_SIZE];
        encode_cpu_result(0, transfer.offset, Ok(()), &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(PreparedTransfer {
            segments,
            bases,
            packet,
            selector: transfer.selector(),
        })
    }

    // no host call or fallible operation may separate a successful stack batch from this commit.
    fn commit_code_transfer(&mut self, prepared: PreparedTransfer) {
        self.publish_segment_context(&prepared.segments, &prepared.bases);
        self.segment_descriptors.commit_accessed(prepared.selector);
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&prepared.packet);
    }

    fn code_transfer_fault(&mut self, error: CodeTransferError) -> Result<(), HostError> {
        let error = match error {
            CodeTransferError::GeneralProtection(code) => CpuMemoryError::GeneralProtection(code),
            CodeTransferError::NotPresent(code) => CpuMemoryError::SegmentNotPresent(code),
            CodeTransferError::UnsupportedProfile => return Err(HostError::InvalidRequest),
        };
        self.code_memory_fault(0, error)
    }

    fn code_memory_fault(&mut self, length: u32, error: CpuMemoryError) -> Result<(), HostError> {
        let mut packet = [0; HELPER_SIZE];
        encode_cpu_result(length, 0, Err(error), &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        Ok(())
    }
}

mod call;
#[path = "far/return.rs"]
mod return_;

#[cfg(test)]
mod support;

#[cfg(test)]
mod tests;
