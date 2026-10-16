use super::{
    super::{
        FileCallOutcome32, FileRuntime32, asynchronous::AsyncKind32, ordinary::memory as guest,
    },
    PipeWait32,
    acceptance::PipeOperation32,
};
use crate::{
    files::FileError32,
    memory::AddressSpace,
    windows::{files::FileApi32 as Api, synchronization::SynchronizationContext32},
};
impl FileRuntime32 {
    pub(super) fn call_named_pipe(
        &mut self,
        memory: &mut AddressSpace,
        mut sync: Option<&mut SynchronizationContext32>,
        context: super::PipeCallContext32<'_>,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let super::PipeCallContext32 { thread, now, token } = context;
        if let Some(PipeWait32::Operation(id)) = self.pipe_wait {
            return self.progress_pipe_id(memory, sync, id);
        }
        let name = guest::path(
            memory,
            args[0],
            api == Api::CallNamedPipeW,
            self.ordinary.ansi,
        )?;
        if !self.pipes.available(&name)? {
            let wait = self.wait_named_pipe(name, args[6], now)?;
            if matches!(
                wait,
                FileCallOutcome32::Pending | FileCallOutcome32::Return { value: 0, .. }
            ) {
                return Ok(wait);
            }
            unreachable!("unavailable pipe cannot complete its name wait")
        }
        let input = self.pipe_input_bytes(memory, args[1], args[2])?;
        let mut client = self.pipes.prepare_client(&name, 0xc0000000, false, token)?;
        client.set_message_read(&self.pipes)?;
        let request = PipeOperation32 {
            pipe: client.endpoint(),
            thread,
            buffer: args[3],
            length: args[4],
            count: args[5],
            overlapped: 0,
            kind: AsyncKind32::Transact {
                input,
                sent: false,
                close: true,
            },
        };
        let id = self.accept_pipe_call(memory, sync.as_deref_mut(), client, request)?;
        self.pipe_wait = Some(PipeWait32::Operation(id));
        self.progress_pipe_id(memory, sync, id)
    }
}
