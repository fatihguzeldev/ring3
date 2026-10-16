mod packet;
mod patch;
mod payload;
mod pipeline;
mod readback;

use super::*;
use crate::graphics::{
    ResourceId,
    d3d8::{
        commands::{Command, ReadbackId, ResourceVersion, SurfaceVersion},
        resources::SurfaceRef,
    },
};

fn source() -> SurfaceVersion {
    SurfaceVersion {
        surface: SurfaceRef {
            resource: ResourceId {
                epoch: 77,
                slot: 3,
                generation: 5,
            },
            subresource: 4,
        },
        version: 2,
    }
}
