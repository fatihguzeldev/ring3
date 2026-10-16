use super::{
    super::{
        DSoundRuntime32,
        memory::{bytes, output, word},
        objects::ObjectKind32,
    },
    Lock32,
};
use crate::{
    audio::{AudioContext, dsound::HResult},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE},
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn lock_buffer(
        &mut self,
        memory: &mut AddressSpace,
        audio: &AudioContext,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, HResult> {
        for out in [args[3], args[4]] {
            output(memory, out, 4)?;
        }
        if (args[5] == 0) != (args[6] == 0) {
            return Err(HResult::INVALID_PARAM);
        }
        if args[5] != 0 {
            output(memory, args[5], 4)?;
            output(memory, args[6], 4)?;
        }
        let object = self.com.preflight(memory, pointer)?;
        let slot = self.slot(pointer)?;
        let (span, data, capture) = match &self.records[slot].as_ref().unwrap().kind {
            ObjectKind32::Buffer(buffer) => {
                let span = if buffer.native.primary {
                    buffer.native.lock_primary_span(
                        audio,
                        &self.device(buffer.device)?.native,
                        args[1],
                        args[2],
                        args[7],
                        args[5] != 0,
                    )?
                } else {
                    buffer
                        .native
                        .lock_span(audio, args[1], args[2], args[7], args[5] != 0)?
                };
                (
                    span,
                    audio
                        .stream_bytes(buffer.native.storage)
                        .map_err(HResult::from)?,
                    false,
                )
            }
            ObjectKind32::CaptureBuffer(buffer) => {
                let span = buffer
                    .native
                    .lock_span(args[1], args[2], args[7], args[5] != 0)?;
                (span, buffer.native.bytes(), true)
            }
            _ => return Err(HResult::INVALID_PARAM),
        };
        let mut staging = Vec::new();
        staging
            .try_reserve_exact((span.first + span.second) as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        staging.extend_from_slice(&data[span.offset as usize..(span.offset + span.first) as usize]);
        staging.extend_from_slice(&data[..span.second as usize]);
        let range = self.locks.available_range(memory, staging.len())?;
        let first = (range.first * PAGE_SIZE as usize) as u32;
        let second = if span.second == 0 {
            0
        } else {
            first
                .checked_add(span.first)
                .ok_or(HResult::INVALID_PARAM)?
        };
        let mut words = [
            word(args[3], first),
            word(args[4], span.first),
            word(args[5], second),
            word(args[6], span.second),
        ];
        let count = if args[5] == 0 { 2 } else { 4 };
        words[..count].sort_unstable_by_key(|word| word.address.0);
        self.locks.install(
            memory,
            range,
            &staging,
            &words[..count],
            |range, mapping| Lock32 {
                object: object.id,
                span,
                range,
                mapping,
                first,
                second,
                original: None,
            },
        )?;
        if capture {
            self.locks.entries.last_mut().unwrap().original = Some(staging);
        }
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn unlock_buffer(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let identity = self.com.preflight(memory, pointer)?;
        let index = self
            .locks
            .entries
            .iter()
            .rposition(|lock| {
                lock.object == identity.id && lock.first == args[1] && lock.second == args[3]
            })
            .ok_or(HResult::INVALID_PARAM)?;
        let lock = &self.locks.entries[index];
        self.locks.validate(memory, lock)?;
        if args[2] > lock.span.first
            || args[4] > lock.span.second
            || memory.transaction_version() == u64::MAX
        {
            return Err(HResult::INVALID_PARAM);
        }
        let mut first = bytes(memory, lock.first, args[2] as usize)?;
        let mut second = bytes(memory, lock.second, args[4] as usize)?;
        let range = lock.range;
        let offset = lock.span.offset;
        let original = lock.original.as_deref();
        let second_at = lock.span.first as usize;
        let slot = self.slot(pointer)?;
        if let ObjectKind32::Buffer(buffer) = &self.records[slot].as_ref().unwrap().kind
            && buffer.native.primary
            && self.device(buffer.device)?.native.cooperative_level != 4
        {
            return Err(HResult::PRIORITY_LEVEL);
        }
        match &mut self.records[slot].as_mut().unwrap().kind {
            ObjectKind32::Buffer(buffer) => {
                if buffer.native.lost {
                    return Err(HResult::BUFFER_LOST);
                }
                audio.write_stream_buffer(buffer.native.storage, offset, &first, &second)?;
            }
            ObjectKind32::CaptureBuffer(buffer) => {
                if let Some(original) = original {
                    // capture may advance while a guest reads its lock snapshot.
                    for (index, value) in first.iter_mut().enumerate() {
                        if *value == original[index] {
                            *value = buffer.native.bytes()[offset as usize + index];
                        }
                    }
                    for (index, value) in second.iter_mut().enumerate() {
                        if *value == original[second_at + index] {
                            *value = buffer.native.bytes()[index];
                        }
                    }
                }
                buffer.native.write_locked(offset, &first, &second)?;
            }
            _ => return Err(HResult::INVALID_PARAM),
        }
        memory
            .unmap(range)
            .expect("sound lock mapping and unmap version were preflighted");
        self.locks.entries.remove(index);
        Ok(0)
    }
}
