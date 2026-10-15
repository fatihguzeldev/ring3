mod callbacks;
mod publication;

use super::super::DispatchError32;
use crate::{
    graphics::{ResourceId, d3d8::resources::PreparedPrivateData},
    memory::GuestAddress,
};

#[derive(Clone, Copy, Debug)]
pub(in crate::process::windows) struct PrivateCallbackRequest32 {
    pub procedure: GuestAddress,
    pub arguments: [u32; 4],
    pub argument_count: u8,
    pub continuation: u64,
}

pub(in crate::process::windows::d3d8) struct PendingPrivateCall {
    continuation: u64,
    action: Option<Action>,
    phase: Phase,
    acquired: Option<GuestAddress>,
    releases: Vec<GuestAddress>,
    result: u32,
}

enum Action {
    Set {
        object: GuestAddress,
        device: GuestAddress,
        resource: ResourceId,
        namespace: u64,
        key: [u8; 16],
        prepared: PreparedPrivateData,
        unknown: Option<GuestAddress>,
    },
    Free {
        object: GuestAddress,
        device: GuestAddress,
        resource: ResourceId,
        namespace: u64,
        key: [u8; 16],
    },
    Get {
        bytes: Vec<u8>,
        output: u32,
        size: u32,
        memory_identity: u64,
        mappings: Vec<(GuestAddress, u64)>,
    },
}

#[derive(Clone, Copy)]
enum Phase {
    Add,
    Release,
    Compensate(DispatchError32),
}
