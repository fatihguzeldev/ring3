use super::*;
use crate::{
    abi::{arena::*, x86::*},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::{
        exceptions::context::{CONTEXT_SIZE32, ContextState32, Win32Context32},
        thread::ThreadApi32 as Api,
    },
};

#[derive(Clone, Copy)]
pub(super) struct ThreadContextOverride32 {
    state: ContextState32,
    depth: usize,
}

#[cfg(feature = "external-cpu")]
impl ThreadRuntime32 {
    pub(in crate::process) fn resumes_in(&self, start: u32, end: u32) -> bool {
        self.threads
            .iter()
            .filter(|thread| thread.state != ThreadState32::Terminated)
            .any(|thread| {
                (thread.id != self.current
                    && thread
                        .saved
                        .as_ref()
                        .is_some_and(|saved| saved.resumes_in(start, end)))
                    || thread
                        .active_invocations
                        .iter()
                        .any(|invocation| invocation.saved.resumes_in(start, end))
                    || thread.context_override.is_some_and(|context| {
                        start <= context.state.state.eip && context.state.state.eip < end
                    })
            })
    }
}
fn invalid() -> HostError {
    HostError::Thread(ThreadError32::InvalidParameter)
}
impl saved::SavedThread32 {
    fn cpu_context(&self) -> Result<ContextState32, HostError> {
        let state = decode_state(&self.arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| invalid())?;
        let segments = decode_current_segment_context(
            &self.arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            Some(
                &self.arena[SEGMENT_BASES_RECORD_OFFSET
                    ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE],
            ),
        )
        .map_err(|_| invalid())?;
        let x87 =
            decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).map_err(|_| invalid())?;
        let simd = decode_simd(&self.arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| invalid())?;
        Ok(ContextState32 {
            state,
            segments,
            x87,
            simd,
            debug: [0; 6],
        })
    }
    fn set_cpu_context(&mut self, context: &ContextState32) -> Result<(), HostError> {
        let mut arena = self.arena;
        encode_state_current(
            &context.state,
            &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
        )
        .map_err(|_| invalid())?;
        encode_x87(&context.x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| invalid())?;
        encode_simd(
            &context.simd,
            &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
        )
        .map_err(|_| invalid())?;
        let (low, high) = arena.split_at_mut(SEGMENT_BASES_RECORD_OFFSET);
        encode_current_segment_context(
            &context.segments,
            &mut low[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            &mut high[..SEGMENT_BASES_RECORD_SIZE],
        )
        .map_err(|_| invalid())?;
        encode_exit_v3(
            &crate::cpu::ExecutionExit {
                retired: 0,
                reason: crate::cpu::ExitReason::NeedCode,
            },
            &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .map_err(|_| invalid())?;
        crate::process::cpu::context::ExtendedCpuState::capture(&arena)?;
        self.arena = arena;
        Ok(())
    }
}
impl EngineInstance {
    pub(super) fn execute_thread_context(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        let id = self.resolve_thread_handle(
            args[0],
            if api == Api::GetThreadContext {
                8
            } else {
                0x10
            },
        )?;
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == id && thread.state != ThreadState32::Terminated)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        let current = id == self.threads.current_id();
        let record = &self.threads.threads[index];
        let base = if let Some(overridden) = record.context_override {
            overridden.state
        } else if current {
            self.windows_context_state()?
        } else {
            record
                .saved
                .as_ref()
                .ok_or(HostError::Thread(ThreadError32::Stale))?
                .cpu_context()?
        };
        if api == Api::GetThreadContext {
            let mut word = [0; 4];
            self.memory()?
                .read(GuestAddress(args[1]), &mut word)
                .map_err(HostError::Memory)?;
            let flags = u32::from_le_bytes(word);
            let context = Win32Context32::capture(flags, &base).map_err(|_| invalid())?;
            let mut spans = [(GuestAddress(0), &[][..]); 6];
            let mut count = 0;
            for (mask, start, end) in [
                (0x10, 4, 28),
                (8, 28, 140),
                (4, 140, 156),
                (2, 156, 180),
                (1, 180, 204),
                (0x20, 204, CONTEXT_SIZE32),
            ] {
                if flags & mask != 0 {
                    spans[count] = (
                        GuestAddress(args[1].checked_add(start as u32).ok_or_else(invalid)?),
                        &context.bytes()[start..end],
                    );
                    count += 1;
                }
            }
            self.memory
                .as_mut()
                .unwrap()
                .write_spans(&spans[..count])
                .map_err(HostError::Memory)?;
            return Ok(1);
        }
        if current {
            return Err(HostError::Thread(ThreadError32::InvalidParameter));
        }
        let mut bytes = [0; CONTEXT_SIZE32];
        self.memory()?
            .read(GuestAddress(args[1]), &mut bytes)
            .map_err(HostError::Memory)?;
        let context = Win32Context32::from_bytes(&bytes).map_err(|_| invalid())?;
        if context.requested_debug_registers(&base) != [0; 6] {
            return Err(HostError::Thread(ThreadError32::Unsupported));
        }
        let candidate = self
            .apply_windows_context(&context, &base)
            .map_err(|_| invalid())?;
        Win32Context32::capture(context.flags(), &candidate).map_err(|_| invalid())?;
        let record = &mut self.threads.threads[index];
        let saved = record
            .saved
            .as_mut()
            .ok_or(HostError::Thread(ThreadError32::Stale))?;
        if saved.blocks_context_change() {
            record.context_override = Some(ThreadContextOverride32 {
                state: candidate,
                depth: record.active_invocations.len(),
            });
        } else {
            saved.set_cpu_context(&candidate)?;
            record.context_override = None;
        }
        Ok(1)
    }
    pub(super) fn apply_deferred_thread_context(&mut self) -> Result<(), HostError> {
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == self.threads.current_id())
            .unwrap();
        let Some(prepared) = self.threads.threads[index].context_override else {
            return Ok(());
        };
        if self.pending_call.is_some()
            || self.cpu_event.is_some()
            || self.exceptions.thread_unhandled(self.threads.current_id())
            || self.threads.threads[index].active_invocations.len() != prepared.depth
        {
            return Ok(());
        }
        self.publish_windows_context(&prepared.state)?;
        encode_exit_v3(
            &crate::cpu::ExecutionExit {
                retired: 0,
                reason: crate::cpu::ExitReason::NeedCode,
            },
            &mut self.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .expect("validated empty CPU exit");
        self.threads.threads[index].context_override = None;
        Ok(())
    }
}
