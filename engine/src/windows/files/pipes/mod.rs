mod channel;
mod context;
mod io;
mod namespace;
mod readiness;
mod security;

use crate::{files::FileError32, windows::security::SecurityDescriptor32};
pub(crate) use context::PipeContext32;
pub(crate) use namespace::pipe_name;
pub(crate) use security::default_security;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PipeEndpointId32(pub u64);
#[derive(Clone, Copy)]
pub(crate) struct PipeLimits32 {
    pub instances: usize,
    pub endpoints: usize,
    pub bytes: usize,
    pub messages: usize,
}
#[derive(Clone, Copy)]
pub(crate) struct PipeOptions32 {
    pub access: u32,
    pub message: bool,
    pub read_message: bool,
    pub nonblocking: bool,
    pub overlapped: bool,
    pub first: bool,
    pub maximum: u32,
    pub in_buffer: usize,
    pub out_buffer: usize,
    pub timeout: u32,
}
impl Default for PipeOptions32 {
    fn default() -> Self {
        Self {
            access: 3,
            message: false,
            read_message: false,
            nonblocking: false,
            overlapped: false,
            first: false,
            maximum: 1,
            in_buffer: 4096,
            out_buffer: 4096,
            timeout: 50,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PipeConnection32 {
    Listening,
    Connected,
    Broken,
}
#[derive(Clone, Copy)]
pub(crate) struct PipeEndpointInfo32 {
    pub server: bool,
    pub message: bool,
    pub read_message: bool,
    pub nonblocking: bool,
    pub overlapped: bool,
    pub in_buffer: u32,
    pub out_buffer: u32,
    pub max_instances: u32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PipeState32 {
    Listening,
    Connected,
    Disconnected,
}
struct Instance32 {
    name: Option<Vec<u16>>,
    server: Option<PipeEndpointId32>,
    client: Option<PipeEndpointId32>,
    session: u64,
    state: PipeState32,
    message: bool,
    access: u32,
    maximum: u32,
    timeout: u32,
    channels: [channel::Channel32; 2],
    security: SecurityDescriptor32,
}
struct Endpoint32 {
    id: PipeEndpointId32,
    instance: usize,
    references: u32,
    client: bool,
    session: u64,
    access: u32,
    read_message: bool,
    nonblocking: bool,
    overlapped: bool,
}
pub(crate) struct PreparedPipeCreate32 {
    revision: u64,
    slot: usize,
    endpoint_slots: [usize; 2],
    instance: Instance32,
    server: Endpoint32,
    client: Option<Endpoint32>,
    capacity: usize,
}
pub(crate) struct PreparedPipeClient32 {
    revision: u64,
    slot: usize,
    endpoint: Endpoint32,
    granted: u32,
}
pub(crate) struct PreparedPipeSecurity32 {
    revision: u64,
    updates: Vec<(usize, SecurityDescriptor32)>,
}
pub(crate) struct PreparedPipeListen32 {
    revision: u64,
    instance: usize,
}
impl PreparedPipeClient32 {
    pub(crate) fn endpoint(&self) -> PipeEndpointId32 {
        self.endpoint.id
    }
    pub(crate) fn overlapped(&self) -> bool {
        self.endpoint.overlapped
    }
    pub(crate) fn set_message_read(&mut self, context: &PipeContext32) -> Result<(), FileError32> {
        if context.revision != self.revision {
            return Err(FileError32::Stale);
        }
        if !context.instances[self.endpoint.instance]
            .as_ref()
            .unwrap()
            .message
        {
            return Err(FileError32::InvalidParameter);
        }
        self.endpoint.read_message = true;
        Ok(())
    }
    pub(crate) fn granted_access(&self) -> u32 {
        self.granted
    }
}
pub(crate) enum PipeIo32<T> {
    Ready(T),
    Pending,
}
pub(crate) struct PreparedPipeRead32 {
    revision: u64,
    instance: usize,
    channel: usize,
    bytes: Vec<u8>,
    more: bool,
    consume_empty: bool,
}
impl PreparedPipeRead32 {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn more(&self) -> bool {
        self.more
    }
}
pub(crate) struct PreparedPipeWrite32 {
    revision: u64,
    instance: usize,
    channel: usize,
    bytes: Vec<u8>,
    replacement: Option<Vec<u8>>,
    message: bool,
}
impl PreparedPipeWrite32 {
    pub(crate) fn length(&self) -> usize {
        self.bytes.len()
    }
}

#[cfg(test)]
mod tests;
