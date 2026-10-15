mod edges;
mod reclamation;
mod swapchains;

use crate::process::windows::d3d8::tests::support::{DATA, read, runtime};
use crate::{
    graphics::{
        ResourceId,
        d3d8::{
            api::InterfaceKind,
            resources::{Format, Pool, ResourceDesc, SurfaceDesc, SurfaceRef, Usage},
        },
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::{
        d3d8::{
            D3D8Runtime32,
            objects::{Object32, ObjectKind32, Resource32},
        },
        objects::{GuestComCreation32, InterfaceLayout32},
    },
    windows::com::ReferenceState32,
};

fn surface_desc() -> ResourceDesc {
    ResourceDesc::Surface(SurfaceDesc {
        width: 4,
        height: 4,
        format: Format::A8R8G8B8,
        usage: Usage(Usage::RENDER_TARGET),
        pool: Pool::Default,
        multisample: 0,
        lockable: true,
    })
}

fn expose_surface(
    runtime: &mut D3D8Runtime32,
    memory: &mut AddressSpace,
    device: GuestAddress,
    resource: ResourceId,
) -> GuestAddress {
    let slot = runtime.vacant_record().unwrap();
    let interface = InterfaceKind::Surface;
    let identity = runtime
        .com
        .create_with_lifetime(
            memory,
            slot as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &runtime.methods[interface as usize - 1],
            }],
            GuestComCreation32 {
                initial: ReferenceState32 {
                    references: 1,
                    holds: 1,
                },
                retain: &[device],
                ..Default::default()
            },
        )
        .unwrap();
    runtime.records[slot] = Some(Object32 {
        pointer: identity.identity_pointer,
        interface,
        kind: ObjectKind32::Resource(Resource32 { device, resource }),
        private_unknowns: Vec::new(),
    });
    identity.identity_pointer
}
