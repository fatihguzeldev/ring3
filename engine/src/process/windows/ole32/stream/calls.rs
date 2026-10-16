use super::{
    context::stream_error,
    io::{check_output, large},
};
use crate::{
    memory::AddressSpace,
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::ole32::{Ole32Runtime32, memory::com_error},
    },
    windows::{
        CallFrame32, CallingConvention32,
        ole32::{HResult32, StreamMethod32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn try_complete_com_stream_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(method) = StreamMethod32::from_id(id) else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != method.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        if self.resume_stream_copy()? {
            return Ok(true);
        }
        if method.0 == 7 {
            let runtime = self.ole32.as_ref().ok_or(HostError::InvalidRequest)?;
            let target = u64::from(frame.arguments()[1]);
            let range = runtime.com.reservation();
            // The entire configured object arena remains engine-owned, including
            // released/stale addresses and identities with another payload.
            if target < range.first as u64 * 4096
                || target >= (range.first + range.count) as u64 * 4096
            {
                let index = runtime.stream_index(self.memory()?, frame.arguments()[0])?;
                let id = runtime.streams.entries[index].unwrap().id;
                self.begin_stream_copy(owner, token, frame, id)?;
                return Ok(true);
            }
        }
        let runtime = self.ole32.as_mut().ok_or(HostError::InvalidRequest)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let index = runtime.stream_index(memory, frame.arguments()[0])?;
        let result = runtime
            .stream_call(memory, index, method, frame.arguments())
            .unwrap_or_else(|error| error.0);
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
        Ok(true)
    }
}
impl Ole32Runtime32 {
    pub(super) fn stream_call(
        &mut self,
        memory: &mut AddressSpace,
        index: usize,
        method: StreamMethod32,
        args: &[u32],
    ) -> Result<u32, HResult32> {
        let entry = self.streams.entries[index].unwrap();
        match method.0 {
            0 => self.query_stream(memory, args[0], args[1], args[2]),
            1 => self.com.add_ref(memory, entry.pointer).map_err(com_error),
            2 => self.release_stream(memory, index),
            3 => self.read_stream(memory, entry.id, args),
            4 => self.write_stream(memory, entry.id, args),
            5 => self.seek_stream(memory, entry.id, args),
            6 => {
                let prepared = self
                    .streams
                    .store
                    .prepare_resize(entry.id, large(args[1], args[2]))
                    .map_err(stream_error)?;
                self.streams.store.commit_resize(prepared);
                Ok(0)
            }
            7 => self.copy_stream(memory, entry.id, args),
            8 if args[1] & !0x1f == 0 => Ok(0),
            8 => Err(HResult32(0x8003_00ff)),
            9 => Ok(0),
            10 | 11 => Err(HResult32(0x8003_0001)),
            12 => self.stat_stream(memory, entry.id, args),
            13 => {
                check_output(memory, args[1], 4)?;
                self.clone_stream(memory, entry.id, args[1])
            }
            _ => Err(HResult32::INVALID_ARGUMENT),
        }
    }
}
