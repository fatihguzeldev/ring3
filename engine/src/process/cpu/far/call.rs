use super::*;
use crate::{cpu::x86::SegmentAccess, memory::MemoryError, process::StoreCompletion};

impl EngineInstance {
    pub fn far_call(
        &mut self,
        offset: u32,
        selector: u32,
        word: u32,
        return_eip: u32,
        esp: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard(self.key, self.generation())?;
        self.far_call_bound(None, offset, selector, word, return_eip, esp)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn far_call_resident(
        &mut self,
        key: u64,
        id: u64,
        offset: u32,
        selector: u32,
        word: u32,
        return_eip: u32,
        esp: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_resident(key, id)?;
        self.far_call_bound(Some(id), offset, selector, word, return_eip, esp)
    }

    #[allow(clippy::too_many_arguments)]
    fn far_call_bound(
        &mut self,
        resident: Option<u64>,
        offset: u32,
        selector: u32,
        word: u32,
        return_eip: u32,
        esp: u32,
    ) -> Result<StoreCompletion, HostError> {
        let width = width(word)?;
        let selector = u16::try_from(selector).map_err(|_| HostError::InvalidRequest)?;
        let context = self.segment_context()?;
        let destination = match self
            .segment_descriptors
            .resolve_far_call(selector, USER_MODE_ENVIRONMENT.current_privilege())
        {
            Ok(destination) => destination,
            Err(error) => {
                self.code_transfer_fault(error)?;
                return Ok(StoreCompletion::Complete);
            }
        };
        let slot = width.slot_bytes();
        let offsets = [esp.wrapping_sub(slot), esp.wrapping_sub(2 * slot)];
        // Intel places stack capacity before the target-offset limit for direct far CALL.
        // our batch profile checks both cached spans before any page permission probe.
        for offset in offsets {
            if let Err(error) = super::super::segments::check_cached_span(
                &context,
                SegmentRegister::Ss,
                offset,
                slot,
                SegmentAccess::Write,
            ) {
                self.code_memory_fault(slot, error)?;
                return Ok(StoreCompletion::Complete);
            }
        }
        let transfer = match destination.target(offset, width) {
            Ok(transfer) => transfer,
            Err(error) => {
                self.code_transfer_fault(error)?;
                return Ok(StoreCompletion::Complete);
            }
        };
        let prepared = self.prepare_code_transfer(context, transfer)?;
        let values = [
            u32::from(context.state.selectors[SegmentRegister::Cs.index()]),
            width.offset(return_eip),
        ];
        let ss_base = context.state.bases[SegmentRegister::Ss.index()];
        let addresses = offsets.map(|offset| ss_base.wrapping_add(offset));
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        if let Err(error) = memory.write_stack(&addresses, &values, slot as usize) {
            let length = if matches!(error, MemoryError::Fault(_)) {
                slot
            } else {
                0
            };
            self.code_memory_fault(length, CpuMemoryError::Memory(error))?;
            return Ok(StoreCompletion::Complete);
        }
        let changed = version != memory.code_validation_version();
        // all encoding and memory preflights completed. This commit cannot fail or reenter.
        self.commit_code_transfer(prepared);
        self.cpu_store_currentness(changed, resident)
    }
}

#[cfg(test)]
mod tests;
