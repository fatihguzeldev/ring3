use super::super::{FileCallOutcome32, FileRuntime32, handles::KernelFileObject32};
use crate::{
    files::FileError32,
    memory::AddressSpace,
    process::windows::threads::ThreadRuntime32,
    windows::files::{
        FileApi32 as Api,
        completion::{CompletionPacket32, CompletionPortId32, CompletionTarget32},
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn completion_target(
        &self,
        handle: u32,
    ) -> Result<CompletionTarget32, FileError32> {
        let target = self.handles.io_target(handle)?;
        let CompletionTarget32::File(open) = target;
        let overlapped = self.vfs.open_options(open)?.overlapped;
        if !overlapped {
            return Err(FileError32::InvalidParameter);
        }
        Ok(target)
    }
    pub(super) fn completion_port(&self, handle: u32) -> Result<CompletionPortId32, FileError32> {
        if self.handles.access(handle)? & 2 == 0 {
            return Err(FileError32::AccessDenied);
        }
        match self.handles.object(handle)? {
            KernelFileObject32::CompletionPort(port) => Ok(port),
            _ => Err(FileError32::InvalidHandle),
        }
    }
    pub(in crate::process::windows::files) fn execute_completion_api(
        &mut self,
        memory: &mut AddressSpace,
        threads: &ThreadRuntime32,
        now: u64,
        thread: u32,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        match api {
            Api::CreateIoCompletionPort => {
                let target = if args[0] == u32::MAX {
                    None
                } else {
                    Some(self.completion_target(args[0])?)
                };
                if args[1] != 0 {
                    let port = self.completion_port(args[1])?;
                    let target = target.ok_or(FileError32::InvalidParameter)?;
                    self.completion.associate(target, port, args[2])?;
                    return Ok(FileCallOutcome32::success(args[1]));
                }
                self.completion.check_create(target)?;
                let handle = self.handles.prepare(0x001f0003, false)?;
                let port = self.completion.create(args[3])?;
                if let Some(target) = target {
                    self.completion
                        .associate(target, port, args[2])
                        .expect("new port association was checked before creation");
                }
                let handle = self
                    .handles
                    .commit(handle, KernelFileObject32::CompletionPort(port));
                Ok(FileCallOutcome32::success(handle))
            }
            Api::PostQueuedCompletionStatus => {
                let port = self.completion_port(args[0])?;
                let packet = CompletionPacket32 {
                    bytes: args[1],
                    key: args[2],
                    overlapped: args[3],
                    status: 0,
                    error: 0,
                };
                let prepared = self.completion.prepare_post(port, packet)?;
                self.completion.commit_post(prepared);
                Ok(FileCallOutcome32::success(1))
            }
            Api::SetFileCompletionNotificationModes => {
                let target = self.completion_target(args[0])?;
                let modes = u8::try_from(args[1]).map_err(|_| FileError32::InvalidParameter)?;
                self.completion.set_modes(target, modes)?;
                Ok(FileCallOutcome32::success(1))
            }
            Api::GetQueuedCompletionStatus | Api::GetQueuedCompletionStatusEx => {
                self.dequeue_completion(memory, threads, now, thread, api, args)
            }
            _ => Err(FileError32::InvalidParameter),
        }
    }
}
