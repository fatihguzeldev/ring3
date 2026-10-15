use crate::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_current_segment_context,
            encode_current_segment_context,
        },
    },
    cpu::x86::{
        CpuReadPort32, SegmentAccess, SegmentContext, SegmentDescriptor, SegmentDescriptorRecord,
        SegmentLoadError, SegmentLoadFailure, SegmentRegister, SegmentState,
    },
    memory::GuestAddress,
};

use crate::process::{CallError, EngineInstance, HostError};

pub(super) fn check_cached_span(
    context: &SegmentContext,
    segment: SegmentRegister,
    offset: u32,
    width: u32,
    access: SegmentAccess,
) -> Result<(), CpuMemoryError> {
    context.caches[segment.index()]
        .check_span(offset, width, access)
        .map_err(|_| {
            if segment == SegmentRegister::Ss {
                CpuMemoryError::StackSegment(0)
            } else {
                CpuMemoryError::GeneralProtection(0)
            }
        })
}

impl EngineInstance {
    pub(in crate::process) fn code_decode_key(
        &self,
    ) -> Result<crate::cpu::x86::decode::CodeDecodeKey, HostError> {
        self.memory()?;
        crate::abi::x86::decode_current_code_key(
            &self.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            Some(
                &self.arena()[SEGMENT_BASES_RECORD_OFFSET
                    ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE],
            ),
        )
        .map_err(|_| HostError::Infrastructure)
    }

    pub(in crate::process) fn code_fetch_context(
        &self,
    ) -> Result<crate::cpu::x86::decode::CodeFetchContext, HostError> {
        self.memory()?;
        let context =
            crate::cpu::x86::decode::CodeFetchContext::from_segments(&self.segment_context()?)
                .ok_or(HostError::Infrastructure)?;
        Ok(context)
    }

    pub fn segment_state(&self) -> Result<SegmentState, HostError> {
        self.segment_context().map(|context| context.state)
    }

    pub fn segment_context(&self) -> Result<SegmentContext, HostError> {
        self.memory()?;
        decode_current_segment_context(
            &self.arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            self.arena.get(
                SEGMENT_BASES_RECORD_OFFSET
                    ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE,
            ),
        )
        .map_err(|_| HostError::Infrastructure)
    }

    pub(in crate::process) fn operand_segment_context(&self) -> Result<SegmentContext, HostError> {
        self.segment_context()
    }

    pub(super) fn publish_segment_context(
        &mut self,
        segments: &[u8; SEGMENTS_SIZE],
        bases: &[u8; SEGMENT_BASES_RECORD_SIZE],
    ) {
        let arena = self.arena.as_mut().get_mut();
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(segments);
        arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(bases);
    }

    pub fn configure_segment_descriptor(
        &mut self,
        descriptor: SegmentDescriptor,
    ) -> Result<(), HostError> {
        self.segment_configuration_guard()?;
        self.segment_descriptors
            .configure(descriptor)
            .map_err(|_| HostError::InvalidRequest)
    }

    pub fn configure_segment_descriptor_raw(
        &mut self,
        selector: u16,
        record: SegmentDescriptorRecord,
    ) -> Result<(), HostError> {
        self.segment_configuration_guard()?;
        self.segment_descriptors
            .configure_raw(selector, record)
            .map_err(|_| HostError::InvalidRequest)
    }

    /// host configuration updates both the visible cache and its known descriptor.
    /// it does not synthesize descriptors for arbitrary guest-supplied selectors.
    pub fn set_segment_base(
        &mut self,
        segment: SegmentRegister,
        base: u32,
    ) -> Result<(), HostError> {
        self.segment_configuration_guard()?;
        let mut context = self.segment_context()?;
        if !context.state.set_base(segment, base) {
            return Err(HostError::InvalidRequest);
        }
        let selector = context.state.selectors[segment.index()];
        let update_descriptor = selector & !3 != 0;
        if update_descriptor {
            let raw = self
                .segment_descriptors
                .raw_descriptor(selector)
                .ok_or(HostError::InvalidRequest)?;
            // only explicit legacy system construction has an opaque compatibility base.
            if !raw.code_data() && self.segment_descriptors.descriptor(selector).is_none() {
                return Err(HostError::InvalidRequest);
            }
        }
        let mut bytes = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut bytes, &mut bases)
            .map_err(|_| HostError::Infrastructure)?;
        if update_descriptor {
            self.segment_descriptors
                .update_base(selector, base)
                .map_err(|_| HostError::InvalidRequest)?;
        }
        self.publish_segment_context(&bytes, &bases);
        Ok(())
    }

    pub fn segment_load(&mut self, target: u32, selector: u32) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.segment_load_bound(target, selector, 0, 0)
    }

    pub fn segment_load_resident(
        &mut self,
        key: u64,
        id: u64,
        target: u32,
        selector: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.segment_load_bound(target, selector, 0, 0)
    }

    pub fn segment_load_far(
        &mut self,
        target: u32,
        address: u32,
        word: u32,
    ) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.segment_load_far_bound(target, address, word)
    }

    pub fn segment_load_far_resident(
        &mut self,
        key: u64,
        id: u64,
        target: u32,
        address: u32,
        word: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.segment_load_far_bound(target, address, word)
    }

    fn segment_configuration_guard(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some()
            || self.callback.is_some()
            || self.has_active_module_operation()
        {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    fn segment_load_far_bound(
        &mut self,
        target: u32,
        address: u32,
        word: u32,
    ) -> Result<(), HostError> {
        register(target)?;
        if word > 1 {
            return Err(HostError::InvalidRequest);
        }
        let length = if word == 1 { 4 } else { 6 };
        let mut bytes = [0; 6];
        if let Err(error) = self
            .cpu_memory_port32()
            .read(GuestAddress(address), &mut bytes[..length])
        {
            return self.segment_packet(length as u32, 0, Err(CpuMemoryError::from(error)));
        }
        let offset_length = length - 2;
        let offset = if word == 1 {
            u32::from(u16::from_le_bytes(bytes[..2].try_into().unwrap()))
        } else {
            u32::from_le_bytes(bytes[..4].try_into().unwrap())
        };
        let selector = u32::from(u16::from_le_bytes(
            bytes[offset_length..length].try_into().unwrap(),
        ));
        self.segment_load_bound(target, selector, length as u32, offset)
    }

    fn segment_load_bound(
        &mut self,
        target: u32,
        selector: u32,
        length: u32,
        value: u32,
    ) -> Result<(), HostError> {
        let target = register(target)?;
        let mut context = self.operand_segment_context()?;
        let cpl = (context.state.selectors[SegmentRegister::Cs.index()] & 3) as u8;
        match self
            .segment_descriptors
            .resolve_load(target, selector as u16, cpl, &context.state)
        {
            Ok(load) => {
                load.apply_context(&mut context, target);
                let mut bytes = [0; SEGMENTS_SIZE];
                let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
                encode_current_segment_context(&context, &mut bytes, &mut bases)
                    .map_err(|_| HostError::Infrastructure)?;
                let result_value = if length == 0 {
                    context.state.bases[target.index()]
                } else {
                    value
                };
                let mut packet = [0; HELPER_SIZE];
                encode_cpu_result(length, result_value, Ok(()), &mut packet)
                    .map_err(|_| HostError::Infrastructure)?;
                self.publish_segment_context(&bytes, &bases);
                self.segment_descriptors.commit_accessed(load.selector);
                self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
                    .copy_from_slice(&packet);
                Ok(())
            }
            Err(error) if error.failure == SegmentLoadFailure::UnsupportedProfile => {
                // profile refusal stays a neutral host rejection, never an invented guest exception.
                Err(HostError::InvalidRequest)
            }
            Err(error) => self.segment_packet(length, 0, Err(segment_error(target, error))),
        }
    }

    fn segment_packet(
        &mut self,
        length: u32,
        value: u32,
        result: Result<(), CpuMemoryError>,
    ) -> Result<(), HostError> {
        encode_cpu_result(
            length,
            value,
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}

fn register(index: u32) -> Result<SegmentRegister, HostError> {
    match index {
        0 => Ok(SegmentRegister::Es),
        1 => Ok(SegmentRegister::Cs),
        2 => Ok(SegmentRegister::Ss),
        3 => Ok(SegmentRegister::Ds),
        4 => Ok(SegmentRegister::Fs),
        5 => Ok(SegmentRegister::Gs),
        _ => Err(HostError::InvalidRequest),
    }
}

fn segment_error(target: SegmentRegister, error: SegmentLoadError) -> CpuMemoryError {
    match error.failure {
        SegmentLoadFailure::NotPresent if target == SegmentRegister::Ss => {
            CpuMemoryError::StackSegment(error.error_code)
        }
        SegmentLoadFailure::NotPresent => CpuMemoryError::SegmentNotPresent(error.error_code),
        _ => CpuMemoryError::GeneralProtection(error.error_code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{abi::arena::TRANSFER_OFFSET, cpu::x86::SegmentDescriptorKind};

    fn engine() -> EngineInstance {
        let mut engine = EngineInstance::new(3, 7).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x2000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
    }

    fn descriptor(selector: u16, base: u32, present: bool) -> SegmentDescriptor {
        SegmentDescriptor {
            selector,
            base,
            present,
            dpl: 3,
            kind: SegmentDescriptorKind::Data { writable: true },
        }
    }

    fn packet(engine: &EngineInstance, offset: usize) -> u32 {
        u32::from_le_bytes(
            engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
                .try_into()
                .unwrap(),
        )
    }

    #[test]
    fn known_reload_null_and_explicit_base_keep_configured_cache() {
        let mut engine = engine();
        engine
            .set_segment_base(SegmentRegister::Fs, 0x5000)
            .unwrap();
        engine.segment_load(4, 0).unwrap();
        assert_eq!(engine.segment_state().unwrap().bases[4], 0x5000);
        engine.segment_load(4, 0x3b).unwrap();
        assert_eq!(engine.segment_state().unwrap().bases[4], 0x5000);
        assert_eq!(packet(&engine, 20), 0x5000);
    }

    #[test]
    fn invalid_and_not_present_selectors_change_only_the_failure_packet() {
        let mut engine = engine();
        for (target, selector, status) in [(4, 0x43, 3), (4, 0x4b, 6), (2, 0x4b, 7)] {
            engine
                .configure_segment_descriptor(descriptor(0x4b, 0, false))
                .unwrap();
            let before = engine.arena().to_vec();
            engine.segment_load(target, selector).unwrap();
            let mut expected = before;
            expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
                .copy_from_slice(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
            assert_eq!(engine.arena(), expected);
            assert_eq!(packet(&engine, 16), status);
            assert_eq!(packet(&engine, 24), selector & !3);
        }
    }

    #[test]
    fn far_pointer_full_span_and_descriptor_checks_precede_publication() {
        let mut engine = engine();
        engine
            .configure_segment_descriptor(descriptor(0x43, 0x6000, true))
            .unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 6]
            .copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0x43, 0]);
        engine.upload(0x2ffc, 4).unwrap();
        let before = engine.segment_state().unwrap();
        engine.segment_load_far(5, 0x2ffc, 0).unwrap();
        assert_eq!(packet(&engine, 4), 0x1000f);
        assert_eq!(packet(&engine, 16), 9);
        assert_eq!(packet(&engine, 24), 4);
        assert_eq!(packet(&engine, 32), 1);
        assert_eq!(packet(&engine, 36), 6);
        assert_eq!(engine.cr2(), Some(GuestAddress(0x3000)));
        assert_eq!(packet(&engine, 28), 0x3000);
        assert_eq!(engine.segment_state().unwrap(), before);
        engine.map(0x3000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 6]
            .copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0x43, 0]);
        engine.upload(0x2ffc, 6).unwrap();
        engine.segment_load_far(5, 0x2ffc, 0).unwrap();
        assert_eq!(packet(&engine, 20), 0x1234_5678);
        assert_eq!(engine.segment_state().unwrap().bases[5], 0x6000);
    }

    #[test]
    fn cached_readonly_descriptor_survives_table_replacement_until_reload() {
        let mut engine = engine();
        let mut readonly = descriptor(0x43, 0x5000, true);
        readonly.kind = SegmentDescriptorKind::Data { writable: false };
        engine.configure_segment_descriptor(readonly).unwrap();
        engine.segment_load(4, 0x43).unwrap();
        let cached = engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].to_vec();
        assert_eq!(&cached[4..6], &2_u16.to_le_bytes());
        assert_eq!(
            u32::from_le_bytes(cached[44..48].try_into().unwrap()),
            0xf31f_ffff
        );
        engine
            .configure_segment_descriptor(descriptor(0x43, 0x6000, true))
            .unwrap();
        assert_eq!(
            &engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            cached
        );
        assert_eq!(engine.segment_state().unwrap().bases[4], 0x5000);
        engine.segment_load(4, 0x43).unwrap();
        assert_eq!(
            &engine.arena()[SEGMENTS_OFFSET + 4..SEGMENTS_OFFSET + 6],
            &1_u16.to_le_bytes()
        );
        assert_eq!(engine.segment_state().unwrap().bases[4], 0x6000);
    }

    #[test]
    fn failed_load_preserves_the_full_cached_record() {
        let mut engine = engine();
        let mut readonly = descriptor(0x43, 0x5000, true);
        readonly.kind = SegmentDescriptorKind::Data { writable: false };
        engine.configure_segment_descriptor(readonly).unwrap();
        engine.segment_load(4, 0x43).unwrap();
        let before = engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].to_vec();
        assert_eq!(&before[4..6], &2_u16.to_le_bytes());
        engine.segment_load(4, 0x4b).unwrap();
        assert_eq!(
            &engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            before
        );
        assert_eq!(packet(&engine, 16), 3);
    }
}

#[cfg(test)]
#[path = "segments/raw/tests.rs"]
mod raw_tests;
