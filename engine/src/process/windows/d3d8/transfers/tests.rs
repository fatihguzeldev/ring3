use super::decoding::copy_regions;
use crate::{
    graphics::d3d8::{api::HResult, resources::Rect},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn zero_copy_count_ignores_guest_arrays_and_nonzero_count_checks_entire_input() {
    let mut memory = AddressSpace::new(2).unwrap();
    assert_eq!(
        copy_regions(&memory, 0xdead_beef, 0xdead_beef, 0, 1024).unwrap(),
        (Vec::new(), None)
    );
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let words = [1u32, 2, 5, 6, 8, 9];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    memory.write(GuestAddress(0x3000), &bytes).unwrap();
    assert_eq!(
        copy_regions(&memory, 0x3000, 0x3010, 1, 1024).unwrap(),
        (
            vec![Rect {
                left: 1,
                top: 2,
                right: 5,
                bottom: 6
            }],
            Some(vec![(8, 9)])
        )
    );
    assert!(copy_regions(&memory, 0x3ff8, 0, 1, 1024).is_err());
    assert_eq!(
        copy_regions(&memory, 0x3000, 0, u32::MAX, 1024),
        Err(HResult::INVALID_CALL)
    );
}

#[test]
fn windowed_front_buffer_requests_adapter_desktop_extent_instead_of_backbuffer_extent() {
    use super::super::{
        DispatchError32,
        tests::support::{DATA, read, runtime},
    };
    use crate::abi::graphics::d3d8::packet;
    use crate::graphics::d3d8::api::{AdapterProfile32, DisplayMode32};
    let (mut runtime, mut memory, device) = runtime();
    let desktop = DisplayMode32 {
        width: 32,
        height: 24,
        refresh_rate: 60,
        format: crate::graphics::d3d8::resources::Format::X8R8G8B8,
    };
    runtime.config.adapter = AdapterProfile32::new(desktop, vec![desktop]).unwrap();
    runtime
        .device_resource_call(&mut memory, device, 27, &[device.0, 32, 24, 21, DATA])
        .unwrap();
    let destination = GuestAddress(read(&memory, DATA));
    let request = match runtime.transfer_call(&mut memory, device, 30, &[device.0, destination.0]) {
        Err(DispatchError32::Readback { id }) => id,
        result => panic!("unexpected result {result:?}"),
    };
    let backend = runtime.device_mut(device).unwrap();
    backend.seal_batch().unwrap();
    let request_bytes = packet(backend.queue().peek().unwrap().1)
        .unwrap()
        .1
        .find(|fragment| fragment.opcode == 14)
        .unwrap()
        .bytes;
    assert_eq!(
        u32::from_le_bytes(request_bytes[60..64].try_into().unwrap()),
        32
    );
    assert_eq!(
        u32::from_le_bytes(request_bytes[64..68].try_into().unwrap()),
        24
    );
    backend
        .complete_readback(request, 1, &[0x5a; 3072])
        .unwrap();
}
