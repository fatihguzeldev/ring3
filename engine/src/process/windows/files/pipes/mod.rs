mod acceptance;
mod calls;
mod cancellation;
mod completion;
mod namespace;
mod queries;
mod transaction;
mod waiting;

use crate::windows::files::pipes::PipeEndpointId32;

#[derive(Clone, Copy)]
pub(super) struct PipeCallContext32<'a> {
    pub thread: u32,
    pub now: u64,
    pub token: &'a crate::windows::security::Token32,
}
pub(super) enum PipeWait32 {
    Operation(u64),
    Name {
        name: Vec<u16>,
        deadline: Option<u64>,
    },
    Flush(PipeEndpointId32),
}

#[cfg(test)]
mod tests;
