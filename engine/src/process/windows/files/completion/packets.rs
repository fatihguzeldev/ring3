use super::super::{FileRuntime32, asynchronous::AsyncOperation32};
use crate::{
    files::FileError32,
    windows::files::completion::{
        CompletionPacket32, CompletionPortId32, CompletionTarget32, PreparedCompletionPost32,
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn prepare_io_completion(
        &self,
        operation: &AsyncOperation32,
        error: u32,
        bytes: u32,
    ) -> Result<Option<PreparedCompletionPost32>, FileError32> {
        self.io_completion_packet(operation, error, bytes)
            .map(|(port, packet)| self.completion.prepare_post(port, packet))
            .transpose()
    }
    pub(in crate::process::windows::files) fn io_completion_packet(
        &self,
        operation: &AsyncOperation32,
        error: u32,
        bytes: u32,
    ) -> Option<(CompletionPortId32, CompletionPacket32)> {
        if operation.skip_port {
            return None;
        }
        let overlapped = operation.overlapped?;
        let target = operation.target.description();
        self.completion.association(target).map(|(port, key, _)| {
            (
                port,
                CompletionPacket32 {
                    bytes,
                    key,
                    overlapped: overlapped.0,
                    status: super::super::asynchronous::status(error),
                    error,
                },
            )
        })
    }
    pub(in crate::process::windows::files) fn retire_completion_file(
        &mut self,
        open: crate::files::OpenFileId,
    ) {
        if self.vfs.open_info(open).is_err() {
            self.completion
                .release_target(CompletionTarget32::File(open));
        }
    }
}
