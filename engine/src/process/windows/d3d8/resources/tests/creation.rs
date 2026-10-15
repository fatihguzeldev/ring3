use super::*;
use crate::{graphics::d3d8::api::HResult, process::windows::d3d8::DispatchError32};

#[test]
fn failed_resource_output_is_neutral_and_mip_surface_description_is_literal_d3d8() {
    let (mut runtime, mut memory, device) = runtime();
    let count = runtime.device(device).unwrap().resources().ids().count();
    assert!(matches!(
        runtime.device_resource_call(
            &mut memory,
            device,
            20,
            &[device.0, 4, 4, 2, 0, 21, 2, 0x8000]
        ),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert_eq!(
        runtime.device(device).unwrap().resources().ids().count(),
        count
    );
    let texture = texture(&mut runtime, &mut memory, device);
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            14,
            &[texture.0, 1, DATA + 32],
        )
        .unwrap();
    let words: Vec<_> = (0..8)
        .map(|index| read(&memory, DATA + 32 + index * 4))
        .collect();
    assert_eq!(words, [21, 1, 0, 2, 16, 0, 2, 2]);
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            15,
            &[texture.0, 1, DATA + 4],
        )
        .unwrap();
    let view = GuestAddress(read(&memory, DATA + 4));
    runtime
        .resource_call(
            &mut memory,
            texture,
            InterfaceKind::Texture,
            15,
            &[texture.0, 1, DATA + 8],
        )
        .unwrap();
    assert_eq!(read(&memory, DATA + 8), view.0);
    assert_eq!(runtime.com.references(texture).unwrap(), 3);
    assert_eq!(runtime.counter_pointer(view).unwrap(), texture);
}
