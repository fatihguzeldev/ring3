use super::super::{call, fixture, memory};
use crate::{
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions},
    windows::gdi::{GdiApi32, GdiError32, LogicalFont32},
};
fn setup() -> (super::super::super::GdiRuntime32, AddressSpace, Vec<u8>) {
    let (runtime, _) = fixture();
    let mut ram = AddressSpace::new(512).unwrap();
    ram.map_zeroed(
        PageRange::new(GuestAddress(0x10000), 2).unwrap(),
        Permissions::READ_WRITE,
    )
    .unwrap();
    let bytes = crate::windows::gdi::private_font_source_for_test();
    ram.map_zeroed(
        PageRange::new(
            GuestAddress(0x100000),
            bytes.len().div_ceil(PAGE_SIZE as usize) as u32,
        )
        .unwrap(),
        Permissions::READ_WRITE,
    )
    .unwrap();
    ram.write(GuestAddress(0x100000), &bytes).unwrap();
    (runtime, ram, bytes)
}
#[test]
fn memory_font_output_fault_is_neutral_and_owned_copy_survives_caller_buffer_and_source_removal() {
    let (mut runtime, mut ram, bytes) = setup();
    for output in [0, 0x19000, u32::MAX - 1] {
        let version = ram.transaction_version();
        assert!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::AddFontMemResourceEx,
                &[0x100000, bytes.len() as u32, 0, output]
            )
            .is_err()
        );
        assert_eq!(ram.transaction_version(), version);
    }
    let resource = call(
        &mut runtime,
        &mut ram,
        GdiApi32::AddFontMemResourceEx,
        &[0x100000, bytes.len() as u32, 0, 0x10100],
    )
    .unwrap();
    assert_eq!(resource, 1);
    assert_eq!(memory::words::<1>(&ram, 0x10100).unwrap(), [1]);
    ram.write(GuestAddress(0x100000), &vec![0; bytes.len()])
        .unwrap();
    let font = runtime
        .create_logical_font(LogicalFont32 {
            face: "Test Mono Face".encode_utf16().collect(),
            ..Default::default()
        })
        .unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, font]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::RemoveFontMemResourceEx,
            &[resource]
        ),
        Ok(1)
    );
    assert_eq!(
        runtime.resolved_dc_font(dc).unwrap().face_name(),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    assert!(runtime.fonts.source_is_live(resource));
}
#[test]
fn memory_font_version_exhaustion_does_not_publish_source_count_or_face() {
    let (mut runtime, mut ram, bytes) = setup();
    memory::write_words(&mut ram, 0x10100, &[0x12345678]).unwrap();
    ram.exhaust_versions_for_test();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::AddFontMemResourceEx,
            &[0x100000, bytes.len() as u32, 0, 0x10100]
        ),
        Err(GdiError32::Memory(
            crate::memory::MemoryError::VersionExhausted
        ))
    );
    assert_eq!(memory::words::<1>(&ram, 0x10100).unwrap(), [0x12345678]);
    assert!(!runtime.fonts.source_is_live(1));
}
