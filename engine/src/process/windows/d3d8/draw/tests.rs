use super::super::tests::support::{DATA, runtime};
use super::decoding::*;
use crate::graphics::d3d8::{api::HResult, resources::Rect, state::Viewport};
use crate::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

#[test]
fn indexed_user_vertices_copy_selected_range_without_reading_unmapped_prefix() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x3000), &[7; 24]).unwrap();
    assert_eq!(
        vertex_bytes(&memory, 0x2000, 1024, 6, 4, 1024).unwrap(),
        [7; 24]
    );
    assert!(vertex_bytes(&memory, 0x2000, 0, 6, 4, 1024).is_err());
    assert_eq!(
        vertex_bytes(&memory, 0xffff_fff0, 1, 1, 32, 1024),
        Err(HResult::INVALID_CALL)
    );
}

#[test]
fn clear_rectangles_clip_signed_coordinates_and_preserve_empty_explicit_lists() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let values = [-20i32, -5, 20, 30, -5, -5, -1, -1];
    let bytes: Vec<_> = values.into_iter().flat_map(i32::to_le_bytes).collect();
    memory.write(GuestAddress(0x3000), &bytes).unwrap();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
        min_z: 0,
        max_z: 1f32.to_bits(),
    };
    assert_eq!(
        clear_rectangles(&memory, 0x3000, 2, viewport, 1024).unwrap(),
        [Rect {
            left: 0,
            top: 0,
            right: 16,
            bottom: 16
        }]
    );
    assert!(
        clear_rectangles(&memory, 0x3010, 1, viewport, 1024)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn guest_indexed_user_draw_owns_selected_memory_before_returning() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_mut(device)
        .unwrap()
        .bind_vertex_shader(4)
        .unwrap();
    runtime
        .draw_call(&mut memory, device, 34, &[device.0])
        .unwrap();
    memory.write(GuestAddress(DATA), &[0x5a; 32]).unwrap();
    memory
        .write(GuestAddress(DATA + 40), &[0, 1, 1, 1])
        .unwrap();
    assert_eq!(
        runtime
            .draw_call(
                &mut memory,
                device,
                73,
                &[device.0, 2, 256, 2, 1, DATA + 40, 101, DATA - 0x1000, 16]
            )
            .unwrap(),
        Some(0)
    );
    memory.write(GuestAddress(DATA), &[0; 32]).unwrap();
    let backend = runtime.device_mut(device).unwrap();
    backend.seal_batch().unwrap();
    assert!(
        backend
            .queue()
            .peek()
            .unwrap()
            .1
            .windows(32)
            .any(|window| window == [0x5a; 32])
    );
    let target = backend.back_buffer(0).unwrap();
    assert_eq!(backend.resources().version(target.resource).unwrap(), 2);
}
