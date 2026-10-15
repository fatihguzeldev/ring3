use super::support::*;
use crate::{
    audio::dsound::{BufferDescription, HResult, InterfaceKind},
    memory::GuestAddress,
    windows::com::IID_IUNKNOWN,
};
#[test]
fn inherited_buffer_identity_separate_interface_counts_and_hidden_device_retention_match_desktop() {
    let (mut runtime, mut memory, mut audio, device) = context();
    let buffer = buffer(
        &mut runtime,
        &mut memory,
        &mut audio,
        device,
        BufferDescription::CONTROL_3D | BufferDescription::NOTIFY,
    );
    memory
        .write(GuestAddress(DATA + 256), &InterfaceKind::Buffer8.iid().0)
        .unwrap();
    assert_eq!(
        runtime.query_interface(&mut memory, buffer, DATA + 256, DATA + 12),
        Ok(0)
    );
    assert_eq!(read(&memory, DATA + 12), buffer.0);
    let vtable = read(&memory, buffer.0);
    assert_eq!(read(&memory, vtable + 21 * 4), 0x9000 + 4 * 0x100 + 21 * 4);
    memory
        .write(GuestAddress(DATA + 256), &InterfaceKind::Buffer3D.iid().0)
        .unwrap();
    runtime
        .query_interface(&mut memory, buffer, DATA + 256, DATA + 16)
        .unwrap();
    let spatial = GuestAddress(read(&memory, DATA + 16));
    assert_eq!(
        runtime.add_ref(&memory, spatial, InterfaceKind::Buffer3D),
        Ok(2)
    );
    assert_eq!(
        runtime.add_ref(&memory, buffer, InterfaceKind::Buffer8),
        Ok(3)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            device,
            InterfaceKind::Device8
        ),
        Ok(0)
    );
    assert!(runtime.object(device).is_ok());
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            buffer,
            InterfaceKind::Buffer8
        ),
        Ok(2)
    );
    assert_eq!(
        runtime.release(&mut memory, &mut audio, None, buffer, InterfaceKind::Buffer),
        Ok(1)
    );
    assert_eq!(
        runtime.release(&mut memory, &mut audio, None, buffer, InterfaceKind::Buffer),
        Ok(0)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            spatial,
            InterfaceKind::Buffer3D
        ),
        Ok(1)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            spatial,
            InterfaceKind::Buffer3D
        ),
        Ok(0)
    );
    assert!(runtime.object(buffer).is_err());
    assert!(runtime.object(device).is_err());
}

#[test]
fn primary_buffer_does_not_expose_buffer8_and_repeated_creation_preserves_identity() {
    let (mut runtime, mut memory, mut audio, device) = context();
    super::super::memory::write_words(
        &mut memory,
        DATA + 128,
        &[36, BufferDescription::PRIMARY, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 8, 0)
        .unwrap();
    let primary = GuestAddress(read(&memory, DATA + 8));
    memory
        .write(GuestAddress(DATA + 256), &InterfaceKind::Buffer8.iid().0)
        .unwrap();
    let before = runtime.com.references(primary).unwrap();
    assert_eq!(
        runtime.query_interface(&mut memory, primary, DATA + 256, DATA + 12),
        Ok(HResult::NO_INTERFACE.0)
    );
    assert_eq!(read(&memory, DATA + 12), 0);
    assert_eq!(runtime.com.references(primary), Ok(before));
    runtime
        .create_buffer(&mut memory, &mut audio, device, DATA + 128, DATA + 16, 0)
        .unwrap();
    assert_eq!(read(&memory, DATA + 16), primary.0);
    assert_eq!(runtime.com.references(primary), Ok(before + 1));
}
#[test]
fn canonical_unknown_counter_is_independent_and_failed_output_never_retains() {
    let (mut runtime, mut memory, mut audio, device) = context();
    memory
        .write(GuestAddress(DATA + 256), &IID_IUNKNOWN.0)
        .unwrap();
    let before = runtime.com.references(device).unwrap();
    assert!(
        runtime
            .query_interface(&mut memory, device, DATA + 256, 0x8000)
            .is_err()
    );
    assert_eq!(runtime.com.references(device), Ok(before));
    runtime
        .query_interface(&mut memory, device, DATA + 256, DATA + 12)
        .unwrap();
    let unknown = GuestAddress(read(&memory, DATA + 12));
    assert_ne!(unknown, device);
    assert_eq!(
        runtime.add_ref(&memory, unknown, InterfaceKind::Unknown),
        Ok(2)
    );
    assert_eq!(
        runtime.add_ref(&memory, device, InterfaceKind::Device8),
        Ok(2)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            unknown,
            InterfaceKind::Unknown
        ),
        Ok(1)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            unknown,
            InterfaceKind::Unknown
        ),
        Ok(0)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            device,
            InterfaceKind::Device8
        ),
        Ok(1)
    );
    assert_eq!(
        runtime.release(
            &mut memory,
            &mut audio,
            None,
            device,
            InterfaceKind::Device8
        ),
        Ok(0)
    );
}
