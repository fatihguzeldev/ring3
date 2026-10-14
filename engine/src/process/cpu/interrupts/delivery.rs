use crate::{
    abi::{
        arena::HELPER_OFFSET,
        cpu_helper::CpuMemoryError,
        exception::encode_exception_result,
        memory_helper::HELPER_SIZE,
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, USER_FLAGS_MASK_V3,
            encode_current_segment_context,
        },
    },
    cpu::x86::{
        CodeTransferError, EnteredEvent32, EventAuthorityKind32, EventDeliveryResult32,
        EventFrame32, EventOrigin32, ExceptionCause32, FrameInput32, SegmentAccess,
        SegmentRegister, SelectorErrorCode32, USER_MODE_ENVIRONMENT, event_secondary_error,
    },
    memory::MemoryError,
    process::{EngineInstance, HostError},
};

#[cfg(any(target_arch = "wasm32", test))]
use super::detection::ConsumedDetection32;
#[cfg(any(target_arch = "wasm32", test))]
use crate::process::calls::call::PendingOwner;

#[derive(Clone, Copy)]
enum SourceMaterial32 {
    Current(Option<u64>),
    #[cfg(any(target_arch = "wasm32", test))]
    Detected(PendingOwner),
}
impl SourceMaterial32 {
    fn is_detected(self) -> bool {
        match self {
            Self::Current(_) => false,
            #[cfg(any(target_arch = "wasm32", test))]
            Self::Detected(_) => true,
        }
    }
}

pub(super) struct PreparedEventResult32 {
    pub(super) result: EventDeliveryResult32,
    pub(super) packet: [u8; HELPER_SIZE],
}

impl EngineInstance {
    /// one trusted detection-time event attempt; no automatic host-exit conversion, retirement or cr2 update.
    pub fn deliver_exception(
        &mut self,
        origin: EventOrigin32,
        input: FrameInput32,
    ) -> Result<EventDeliveryResult32, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.deliver_exception_bound(None, origin, input)
    }

    pub fn deliver_exception_resident(
        &mut self,
        key: u64,
        id: u64,
        origin: EventOrigin32,
        input: FrameInput32,
    ) -> Result<EventDeliveryResult32, HostError> {
        self.guard_material_resident(key, id)?;
        self.deliver_exception_bound(Some(id), origin, input)
    }

    fn deliver_exception_bound(
        &mut self,
        resident: Option<u64>,
        origin: EventOrigin32,
        input: FrameInput32,
    ) -> Result<EventDeliveryResult32, HostError> {
        let prepared =
            self.attempt_exception_entry(SourceMaterial32::Current(resident), origin, input)?;
        Ok(self.publish_prepared_exception(prepared))
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) fn attempt_detected_exception(
        &mut self,
        record: &ConsumedDetection32,
        origin: EventOrigin32,
        input: FrameInput32,
    ) -> Result<PreparedEventResult32, HostError> {
        self.attempt_exception_entry(SourceMaterial32::Detected(record.owner()), origin, input)
    }

    fn attempt_exception_entry(
        &mut self,
        source: SourceMaterial32,
        origin: EventOrigin32,
        input: FrameInput32,
    ) -> Result<PreparedEventResult32, HostError> {
        if input.flags & 2 == 0
            || input.flags & !USER_FLAGS_MASK_V3 != 0
            || USER_MODE_ENVIRONMENT.alignment_mask_enabled()
        {
            return Err(HostError::InvalidRequest);
        }
        let table = self
            .interrupt_descriptors
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let cpl = USER_MODE_ENVIRONMENT.current_privilege();
        let gate = match table.resolve_event_gate(origin, cpl) {
            Ok(gate) => gate,
            Err(CodeTransferError::UnsupportedProfile) => {
                let record = table.record(origin.vector());
                if record.descriptor_type() != 5 {
                    return Err(HostError::InvalidRequest);
                }
                let selector = (record.to_words().0 >> 16) as u16;
                return Self::prepare_event_result(
                    origin,
                    0,
                    EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                        kind: EventAuthorityKind32::Task,
                        selector,
                    },
                );
            }
            Err(error) => return Self::prepare_event_transfer_error(origin, 0, error),
        };
        let info = gate.frame_info()
            | if origin.error_code().is_some() {
                0x200
            } else {
                0
            };
        let destination = match gate.event_destination(&self.segment_descriptors, cpl, origin) {
            Ok(destination) => destination,
            Err(CodeTransferError::UnsupportedProfile) => {
                let record = self
                    .segment_descriptors
                    .raw_descriptor(gate.selector())
                    .ok_or(HostError::Infrastructure)?;
                if !record.code_data() || record.descriptor_type() & 0xc != 8 || record.dpl() >= cpl
                {
                    return Err(HostError::InvalidRequest);
                }
                return Self::prepare_event_result(
                    origin,
                    info,
                    EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                        kind: EventAuthorityKind32::Privilege,
                        selector: gate.selector(),
                    },
                );
            }
            Err(error) => return Self::prepare_event_transfer_error(origin, info, error),
        };
        let mut context = self.segment_context()?;
        if input.cs != context.state.selectors[SegmentRegister::Cs.index()] {
            return Err(HostError::InvalidRequest);
        }
        let frame = EventFrame32::new(
            origin,
            input,
            gate.width(),
            context.caches[SegmentRegister::Ss.index()].default32(),
            gate.frame_info() & 0x100 != 0,
        );
        for &offset in frame.offsets() {
            if let Err(error) = super::super::segments::check_cached_span(
                &context,
                SegmentRegister::Ss,
                offset,
                frame.slot(),
                SegmentAccess::Write,
            ) {
                let CpuMemoryError::StackSegment(_) = error else {
                    return Err(HostError::Infrastructure);
                };
                return Self::prepare_event_result(
                    origin,
                    info,
                    EventDeliveryResult32::ArchitecturalSecondary(ExceptionCause32::StackSegment(
                        SelectorErrorCode32::null(origin.external()),
                    )),
                );
            }
        }
        let transfer = match destination.target(gate.offset(), gate.width()) {
            Ok(transfer) => transfer,
            Err(CodeTransferError::UnsupportedProfile) => return Err(HostError::InvalidRequest),
            Err(error) => {
                return Self::prepare_event_transfer_error(
                    origin,
                    info,
                    event_secondary_error(origin, error),
                );
            }
        };
        let ss_base = context.state.bases[SegmentRegister::Ss.index()];
        let mut addresses = [0; 4];
        for (address, offset) in addresses.iter_mut().zip(frame.offsets()) {
            *address = ss_base.wrapping_add(*offset);
        }
        transfer.apply(&mut context);
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases)
            .map_err(|_| HostError::Infrastructure)?;
        let mut entered = EnteredEvent32 {
            target: transfer.offset,
            esp: frame.esp(),
            flags: frame.flags(),
            frame_info: frame.info(),
            code_invalidated: false,
        };
        let mut prepared =
            Self::prepare_event_result(origin, info, EventDeliveryResult32::Entered(entered))?;
        // retain physical material identity without consulting the new cs interpretation after commit.
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let region = match source {
            SourceMaterial32::Current(Some(id)) => self
                .resident
                .as_ref()
                .ok_or(HostError::InvalidArtifact)?
                .get_raw(memory, id)
                .map_err(HostError::Resident)?,
            #[cfg(any(target_arch = "wasm32", test))]
            SourceMaterial32::Detected(PendingOwner::Resident(id))
            | SourceMaterial32::Detected(PendingOwner::ResidentCallback { unit_id: id, .. }) => {
                self.resident
                    .as_ref()
                    .ok_or(HostError::InvalidArtifact)?
                    .get_retained_raw(memory, id)
                    .map_err(HostError::Resident)?
            }
            SourceMaterial32::Current(None) => {
                self.artifact.as_ref().ok_or(HostError::InvalidArtifact)?
            }
            #[cfg(any(target_arch = "wasm32", test))]
            SourceMaterial32::Detected(PendingOwner::Replacement(_)) => {
                self.artifact.as_ref().ok_or(HostError::InvalidArtifact)?
            }
        };
        let already_invalidated = source.is_detected() && region.wasm_bytes(memory).is_err();
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        if let Err(error) = memory.cpu_write_stack(
            &addresses[..frame.values().len()],
            frame.values(),
            frame.slot() as usize,
        ) {
            let result = match error {
                MemoryError::Fault(fault) => EventDeliveryResult32::NeedsPagingAuthority {
                    fault,
                    length: frame.slot(),
                },
                other => EventDeliveryResult32::Infrastructure(other),
            };
            return Self::prepare_event_result(origin, info, result);
        }
        entered.code_invalidated = already_invalidated
            || (version != memory.code_validation_version() && region.wasm_bytes(memory).is_err());
        // no fallible encoding or host reentry separates frame, code-cache and packet publication.
        self.publish_segment_context(&segments, &bases);
        self.segment_descriptors
            .commit_accessed(transfer.selector());
        prepared.result = EventDeliveryResult32::Entered(entered);
        Ok(prepared)
    }

    fn prepare_event_transfer_error(
        origin: EventOrigin32,
        info: u32,
        error: CodeTransferError,
    ) -> Result<PreparedEventResult32, HostError> {
        let cause = match error {
            CodeTransferError::GeneralProtection(code) => ExceptionCause32::GeneralProtection(
                SelectorErrorCode32::from_bits(code).ok_or(HostError::Infrastructure)?,
            ),
            CodeTransferError::NotPresent(code) => ExceptionCause32::SegmentNotPresent(
                SelectorErrorCode32::from_bits(code).ok_or(HostError::Infrastructure)?,
            ),
            CodeTransferError::UnsupportedProfile => return Err(HostError::InvalidRequest),
        };
        Self::prepare_event_result(
            origin,
            info,
            EventDeliveryResult32::ArchitecturalSecondary(cause),
        )
    }

    fn prepare_event_result(
        origin: EventOrigin32,
        info: u32,
        result: EventDeliveryResult32,
    ) -> Result<PreparedEventResult32, HostError> {
        let mut packet = [0; HELPER_SIZE];
        encode_exception_result(origin, info, &result, &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(PreparedEventResult32 { result, packet })
    }
    pub(super) fn publish_prepared_exception(
        &mut self,
        prepared: PreparedEventResult32,
    ) -> EventDeliveryResult32 {
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&prepared.packet);
        prepared.result
    }
}

#[cfg(test)]
mod tests;
