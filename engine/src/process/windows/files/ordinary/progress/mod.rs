mod calls;
mod cleanup;
mod setup;

use crate::{
    files::{FileId, OpenFileId},
    memory::GuestAddress,
    process::{WindowsCallback32, WindowsCallbackProvider32},
};

#[derive(Clone, Copy)]
pub(super) enum CopyStage32 {
    Start,
    Finish,
}

#[derive(Clone, Copy)]
pub(super) struct PendingCopy32 {
    continuation: u64,
    procedure: GuestAddress,
    data: u32,
    cancel: u32,
    source: OpenFileId,
    destination: OpenFileId,
    source_file: FileId,
    destination_file: FileId,
    source_handle: u32,
    destination_handle: u32,
    size: u64,
    stage: CopyStage32,
}
impl PendingCopy32 {
    fn callback(self) -> WindowsCallback32 {
        let transferred = match self.stage {
            CopyStage32::Start => 0,
            CopyStage32::Finish => self.size,
        };
        let mut arguments = [0; 16];
        for (index, value) in [self.size, transferred, self.size, transferred]
            .into_iter()
            .enumerate()
        {
            arguments[index * 2] = value as u32;
            arguments[index * 2 + 1] = (value >> 32) as u32;
        }
        arguments[8..13].copy_from_slice(&[
            1,
            match self.stage {
                CopyStage32::Start => 1,
                CopyStage32::Finish => 0,
            },
            self.source_handle,
            self.destination_handle,
            self.data,
        ]);
        WindowsCallback32 {
            procedure: self.procedure,
            arguments,
            count: 13,
            continuation: self.continuation,
            provider: WindowsCallbackProvider32::Files,
        }
    }
}
