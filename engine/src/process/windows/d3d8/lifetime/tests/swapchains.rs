use super::*;
use crate::{
    graphics::d3d8::resources::ResourceError,
    process::windows::d3d8::{garbage::Garbage32, objects::SwapChain32},
};

#[test]
fn swapchain_disposal_deduplicates_orphan_storage_and_preserves_external_backbuffer() {
    let (mut runtime, mut memory, device) = runtime();
    let prepared = runtime
        .device(device)
        .unwrap()
        .prepare_resources(&[surface_desc(), surface_desc()])
        .unwrap();
    let ids = prepared.ids().to_vec();
    runtime
        .device_mut(device)
        .unwrap()
        .commit_resources(prepared)
        .unwrap();
    let external = expose_surface(&mut runtime, &mut memory, device, ids[1]);
    let slot = runtime.vacant_record().unwrap();
    let interface = InterfaceKind::SwapChain;
    let identity = runtime
        .com
        .create_with_lifetime(
            &mut memory,
            slot as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &runtime.methods[interface as usize - 1],
            }],
            GuestComCreation32 {
                retain: &[device],
                ..Default::default()
            },
        )
        .unwrap();
    let ObjectKind32::Device(owner) = &runtime.object(device).unwrap().kind else {
        unreachable!()
    };
    let presentation = owner.presentation;
    runtime.records[slot] = Some(Object32 {
        pointer: identity.identity_pointer,
        interface,
        private_unknowns: Vec::new(),
        kind: ObjectKind32::SwapChain(SwapChain32 {
            device,
            presentation,
            surfaces: vec![external],
            resources: vec![
                SurfaceRef {
                    resource: ids[0],
                    subresource: 0,
                },
                SurfaceRef {
                    resource: ids[0],
                    subresource: 0,
                },
                SurfaceRef {
                    resource: ids[1],
                    subresource: 0,
                },
            ],
        }),
    });
    runtime
        .release(&mut memory, identity.identity_pointer)
        .unwrap();
    assert_eq!(
        runtime.garbage,
        [Garbage32::Resource {
            device,
            resource: ids[0]
        }]
    );
    assert_eq!(runtime.com.references(external), Ok(1));
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.device(device).unwrap().resources().desc(ids[0]),
        Err(ResourceError::StaleResource)
    );
    assert!(
        runtime
            .device(device)
            .unwrap()
            .resources()
            .desc(ids[1])
            .is_ok()
    );
    runtime.release(&mut memory, external).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.device(device).unwrap().resources().desc(ids[1]),
        Err(ResourceError::StaleResource)
    );
}
