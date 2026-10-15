mod backpressure;
mod ordering;
mod preparation;

use super::*;
use crate::{abi::graphics::d3d8::packet, graphics::ResourceId};

fn queue(bytes: u32) -> CommandQueue {
    CommandQueue::new(
        0x1122_3344_5566_7788,
        CommandLimits {
            batch_bytes: bytes,
            pending_bytes: 64 * 1024,
        },
    )
    .unwrap()
}

fn resource(version: u64) -> ResourceVersion {
    ResourceVersion {
        id: ResourceId {
            epoch: 77,
            slot: 3,
            generation: 5,
        },
        version,
    }
}
