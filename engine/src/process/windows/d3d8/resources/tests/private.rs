use super::*;
use crate::{
    graphics::d3d8::{api::HResult, device::DeviceError},
    process::windows::d3d8::DispatchError32,
};

#[test]
fn private_data_namespaces_and_owned_unknown_output_reference_are_independent() {
    let (mut runtime, mut memory, device) = runtime();
    let texture = texture(&mut runtime, &mut memory, device);
    let guid = DATA + 128;
    memory.write(GuestAddress(guid), &[3; 16]).unwrap();
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            4,
            &[texture.0, guid, device.0, 4, 1],
        )
        .unwrap();
    let refs = runtime.com.references(device).unwrap();
    memory
        .write(GuestAddress(DATA + 64), &4u32.to_le_bytes())
        .unwrap();
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            5,
            &[texture.0, guid, DATA + 68, DATA + 64],
        )
        .unwrap();
    assert_eq!(read(&memory, DATA + 68), device.0);
    assert_eq!(runtime.com.references(device).unwrap(), refs + 1);
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            15,
            &[texture.0, 0, DATA + 4],
        )
        .unwrap();
    let view = GuestAddress(read(&memory, DATA + 4));
    assert!(matches!(
        runtime.resource_call(
            &mut memory,
            view,
            InterfaceKind::Surface,
            5,
            &[view.0, guid, 0, 0]
        ),
        Err(DispatchError32::Device(DeviceError::Resource(
            crate::graphics::d3d8::resources::ResourceError::NotFound
        )))
    ));
    memory
        .write(GuestAddress(DATA + 64), &1u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        runtime
            .resource_call(
                &mut memory,
                texture,
                InterfaceKind::Texture,
                5,
                &[texture.0, guid, 0, DATA + 64]
            )
            .unwrap(),
        Some(0)
    );
    assert_eq!(read(&memory, DATA + 64), 4);
    memory
        .write(GuestAddress(DATA + 64), &1u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        runtime
            .resource_call(
                &mut memory,
                texture,
                InterfaceKind::Texture,
                5,
                &[texture.0, guid, DATA + 68, DATA + 64]
            )
            .unwrap(),
        Some(HResult::MORE_DATA.0)
    );
}

#[test]
fn external_unknown_addref_continuation_compensates_stale_output_with_release() {
    let (mut runtime, mut memory, device) = runtime();
    let texture = texture(&mut runtime, &mut memory, device);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x8000), &0x8100u32.to_le_bytes())
        .unwrap();
    for (index, address) in [0x9000u32, 0x9010, 0x9020].into_iter().enumerate() {
        memory
            .write(
                GuestAddress(0x8100 + index as u32 * 4),
                &address.to_le_bytes(),
            )
            .unwrap();
    }
    let guid = DATA + 128;
    memory.write(GuestAddress(guid), &[4; 16]).unwrap();
    let Err(DispatchError32::Callback(add)) = runtime.resource_call(
        &mut memory,
        texture,
        InterfaceKind::Texture,
        4,
        &[texture.0, guid, 0x8000, 4, 1],
    ) else {
        panic!("expected external addref")
    };
    assert_eq!((add.procedure.0, add.arguments[0]), (0x9010, 0x8000));
    assert_eq!(
        runtime
            .resume_private_callback(add.continuation, 2, &mut memory)
            .unwrap(),
        0
    );
    memory
        .write(GuestAddress(DATA + 64), &4u32.to_le_bytes())
        .unwrap();
    let Err(DispatchError32::Callback(get)) = runtime.resource_call(
        &mut memory,
        texture,
        InterfaceKind::Texture,
        5,
        &[texture.0, guid, DATA + 68, DATA + 64],
    ) else {
        panic!("expected getter addref")
    };
    memory
        .unmap(PageRange::new(GuestAddress(DATA), 1).unwrap())
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let Err(DispatchError32::Callback(release)) =
        runtime.resume_private_callback(get.continuation, 3, &mut memory)
    else {
        panic!("expected compensating release")
    };
    assert_eq!(release.procedure.0, 0x9020);
    assert!(
        runtime
            .resume_private_callback(release.continuation, 2, &mut memory)
            .is_err()
    );
    assert_eq!(read(&memory, DATA + 68), 0);
}
