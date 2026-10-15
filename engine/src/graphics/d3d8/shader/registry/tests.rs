use super::super::{Fvf, ShaderError};
use super::*;
use std::sync::Arc;

const DECLARATION: [u32; 3] = [0x2000_0000, 0x4002_0000, u32::MAX];
const VERTEX: [u32; 5] = [0xfffe_0101, 1, 0xc00f_0000, 0x90e4_0000, 0xffff];
const PIXEL: [u32; 5] = [0xffff_0104, 1, 0x800f_0000, 0x90e4_0000, 0xffff];

fn registry(objects: u32) -> ShaderRegistry {
    ShaderRegistry::new(ShaderLimits {
        objects,
        bytes: 1024 * 1024,
    })
    .unwrap()
}

#[test]
fn shader_handles_never_alias_valid_fvf_and_deleted_handles_stay_stale() {
    let mut registry = registry(2);
    let first = registry
        .create_vertex(&DECLARATION, Some(&VERTEX), 0)
        .unwrap();
    let valid_high_bit_fvf = 2 | 0x800 | (2 << 30);
    assert!(Fvf::parse(valid_high_bit_fvf).is_ok());
    assert!(!is_shader_handle(valid_high_bit_fvf));
    assert!(is_shader_handle(first));
    assert_eq!(Fvf::parse(first).unwrap_err(), ShaderError::InvalidFvf);
    assert_eq!(
        registry.acquire_pixel(first).unwrap_err(),
        RegistryError::InvalidHandle
    );
    drop(registry.delete_vertex(first).unwrap());
    let second = registry
        .create_vertex(&DECLARATION, Some(&VERTEX), 0)
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        registry.acquire_vertex(first).unwrap_err(),
        RegistryError::InvalidHandle
    );
    assert_eq!(
        registry
            .vertex(second)
            .unwrap()
            .function
            .as_ref()
            .unwrap()
            .tokens(),
        VERTEX
    );
}

#[test]
fn deleted_shader_storage_remains_budgeted_until_captured_references_drop() {
    let mut registry = registry(1);
    let handle = registry.create_pixel(&PIXEL).unwrap();
    let captured = registry.acquire_pixel(handle).unwrap();
    let bytes = registry.retained_bytes();
    let removed = registry.delete_pixel(handle).unwrap();
    assert!(Arc::ptr_eq(&captured, &removed));
    drop(removed);
    assert_eq!(registry.live_objects(), 0);
    assert_eq!(registry.retained_bytes(), bytes);
    assert_eq!(
        registry.create_pixel(&PIXEL),
        Err(RegistryError::ObjectCapacity)
    );
    assert_eq!(captured.function.tokens(), PIXEL);
    drop(captured);
    assert_eq!(registry.retained_bytes(), 0);
    assert!(registry.create_pixel(&PIXEL).is_ok());
}

#[test]
fn declaration_only_shader_defers_constant_application_until_binding() {
    let mut registry = registry(2);
    let declaration = [0x2000_0000, 0x4002_0000, 0x8200_0003, 1, 2, 3, 4, u32::MAX];
    let handle = registry.create_vertex(&declaration, None, 0).unwrap();
    let shader = registry.acquire_vertex(handle).unwrap();
    assert!(shader.function.is_none());
    assert_eq!(shader.declaration.tokens(), declaration);
    assert_eq!(
        shader.bind_constants().collect::<Vec<_>>(),
        [(3, [1, 2, 3, 4])]
    );
    let function = [
        0xfffe_0101,
        81,
        0xa00f_0003,
        5,
        6,
        7,
        8,
        1,
        0xc00f_0000,
        0x90e4_0000,
        0xffff,
    ];
    let handle = registry
        .create_vertex(&declaration, Some(&function), 0)
        .unwrap();
    let shader = registry.vertex(handle).unwrap();
    assert_eq!(
        shader.bind_constants().collect::<Vec<_>>(),
        [(3, [1, 2, 3, 4]), (3, [5, 6, 7, 8])]
    );
    assert_eq!(
        shader
            .function
            .as_ref()
            .unwrap()
            .executable_instructions()
            .count(),
        1
    );
}

#[test]
fn invalid_creation_and_handle_exhaustion_do_not_publish_partial_objects() {
    let mut registry = registry(2);
    assert_eq!(
        registry.create_vertex(&DECLARATION, Some(&VERTEX), 1),
        Err(RegistryError::InvalidUsage)
    );
    assert_eq!(registry.live_objects(), 0);
    registry.next_sequence_for_test(0x3fff_ffff);
    let last = registry.create_pixel(&PIXEL).unwrap();
    assert_eq!(last, u32::MAX);
    assert_eq!(
        registry.create_pixel(&PIXEL),
        Err(RegistryError::IdentityExhausted)
    );
    assert_eq!(registry.live_objects(), 1);
    assert_eq!(registry.pixel(last).unwrap().function.tokens(), PIXEL);
}
