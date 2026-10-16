mod context;
mod queues;

use crate::{
    files::{FileError32, OpenFileId},
    windows::files::pipes::PipeEndpointId32,
};
pub(crate) use context::CompletionContext32;

pub(crate) fn completion_access(raw: u32) -> Result<u32, FileError32> {
    let access = crate::windows::security::GenericMapping32 {
        read: 0x20001,
        write: 0x20002,
        execute: 0x120000,
        all: 0x1f0003,
    }
    .map(raw);
    if access & !0x1f0003 != 0 {
        return Err(FileError32::InvalidParameter);
    }
    Ok(access)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompletionPortId32(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompletionTarget32 {
    File(OpenFileId),
    Pipe(PipeEndpointId32),
    Null(crate::windows::files::devices::DeviceDescriptionId32),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CompletionPacket32 {
    pub bytes: u32,
    pub key: u32,
    pub overlapped: u32,
    pub status: u32,
    pub error: u32,
}
struct Port32 {
    id: CompletionPortId32,
    references: u32,
    concurrency: u32,
    head: Option<usize>,
    tail: Option<usize>,
    queued: usize,
}
struct Association32 {
    target: CompletionTarget32,
    port: Option<CompletionPortId32>,
    key: u32,
    modes: u8,
}
struct PacketSlot32 {
    packet: CompletionPacket32,
    next: Option<usize>,
}
#[derive(Clone, Copy)]
struct Thread32 {
    id: u32,
    port: CompletionPortId32,
    active: bool,
    waiting: Option<u64>,
}
pub(crate) struct PreparedCompletionPost32 {
    revision: u64,
    port: CompletionPortId32,
    slot: Option<usize>,
    packet: CompletionPacket32,
}
pub(crate) struct PreparedCompletionPosts32 {
    revision: u64,
    packets: Vec<(CompletionPortId32, usize, CompletionPacket32)>,
}
pub(crate) struct PreparedCompletionTake32 {
    revision: u64,
    port: CompletionPortId32,
    thread: u32,
    row: usize,
    packets: Vec<CompletionPacket32>,
}
impl PreparedCompletionTake32 {
    pub(crate) fn packets(&self) -> &[CompletionPacket32] {
        &self.packets
    }
}
pub(crate) struct PreparedCompletionWait32 {
    revision: u64,
    port: CompletionPortId32,
    thread: u32,
    row: usize,
    order: u64,
}
#[cfg(test)]
mod tests;
