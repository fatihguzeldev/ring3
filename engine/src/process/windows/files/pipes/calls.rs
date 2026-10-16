use super::super::{
    FileCallOutcome32, FileRuntime32, asynchronous::AsyncKind32, ordinary::memory as guest,
};
use super::{PipeWait32, acceptance::PipeOperation32};
use crate::{
    files::{FileAccess32, FileError32},
    memory::AddressSpace,
    windows::{
        files::{FileApi32 as Api, pipes::PipeConnection32},
        synchronization::SynchronizationContext32,
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_pipe_api(
        &mut self,
        memory: &mut AddressSpace,
        mut sync: Option<&mut SynchronizationContext32>,
        context: super::PipeCallContext32<'_>,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let super::PipeCallContext32 { thread, now, token } = context;
        match api {
            Api::CallNamedPipeA | Api::CallNamedPipeW => {
                self.call_named_pipe(memory, sync, context, api, args)
            }
            Api::CreatePipe => self.create_anonymous_pipe(memory, args, token),
            Api::CreateNamedPipeA | Api::CreateNamedPipeW => {
                self.create_named_pipe(memory, api == Api::CreateNamedPipeW, args, token)
            }
            Api::DisconnectNamedPipe => {
                let pipe = self.pipe_handle(args[0], 0)?;
                self.pipes.disconnect(pipe)?;
                Ok(FileCallOutcome32::success(1))
            }
            Api::ConnectNamedPipe | Api::ReadFile | Api::WriteFile | Api::TransactNamedPipe => {
                if let Some(PipeWait32::Operation(id)) = self.pipe_wait {
                    return self.progress_pipe_id(memory, sync, id);
                }
                let (rights, buffer, length, count, overlap, kind) = match api {
                    Api::ReadFile => (
                        FileAccess32::READ.0,
                        args[1],
                        args[2],
                        args[3],
                        args[4],
                        AsyncKind32::Read,
                    ),
                    Api::WriteFile => (
                        FileAccess32::WRITE.0,
                        0,
                        args[2],
                        args[3],
                        args[4],
                        AsyncKind32::Write(self.pipe_input_bytes(memory, args[1], args[2])?),
                    ),
                    Api::TransactNamedPipe => (
                        FileAccess32::READ.0 | FileAccess32::WRITE.0,
                        args[3],
                        args[4],
                        args[5],
                        args[6],
                        AsyncKind32::Transact {
                            input: self.pipe_input_bytes(memory, args[1], args[2])?,
                            sent: false,
                            close: false,
                        },
                    ),
                    _ => (0, 0, 0, 0, args[1], AsyncKind32::Connect),
                };
                let pipe = self.pipe_handle(args[0], rights)?;
                let options = self.pipes.options(pipe)?;
                let listen = if api == Api::ConnectNamedPipe {
                    match self.pipes.connection(pipe)? {
                        PipeConnection32::Connected => return Ok(FileCallOutcome32::error(0, 535)),
                        PipeConnection32::Broken => return Err(FileError32::NoData),
                        PipeConnection32::Listening => {}
                    }
                    if options.nonblocking {
                        self.pipes.listen(pipe)?;
                        return Err(FileError32::PipeListening);
                    }
                    Some(self.pipes.prepare_listen(pipe)?)
                } else {
                    None
                };
                if api == Api::TransactNamedPipe && (!options.message || !options.read_message) {
                    return Err(FileError32::InvalidParameter);
                }
                let request = PipeOperation32 {
                    pipe,
                    thread,
                    buffer,
                    length,
                    count,
                    overlapped: overlap,
                    kind,
                };
                let id = self.accept_pipe_operation(memory, sync.as_deref_mut(), request)?;
                if let Some(listen) = listen {
                    self.pipes.commit_listen(listen);
                }
                if options.overlapped {
                    return Ok(FileCallOutcome32::error(0, 997));
                }
                self.pipe_wait = Some(PipeWait32::Operation(id));
                self.progress_pipe_id(memory, sync, id)
            }
            Api::PeekNamedPipe
            | Api::GetNamedPipeInfo
            | Api::SetNamedPipeHandleState
            | Api::GetNamedPipeHandleStateA
            | Api::GetNamedPipeHandleStateW => self.query_pipe(memory, api, args, token),
            Api::WaitNamedPipeA | Api::WaitNamedPipeW => {
                let name = guest::path(
                    memory,
                    args[0],
                    api == Api::WaitNamedPipeW,
                    self.ordinary.ansi,
                )?;
                self.wait_named_pipe(name, args[1], now)
            }
            _ => Err(FileError32::Unsupported),
        }
    }
}
