use super::{call, fixture, memory};
use crate::{
    memory::GuestAddress,
    windows::gdi::{GdiApi32, GdiError32},
};

fn info(memory: &mut crate::memory::AddressSpace, address: u32, bits: u16, masks: [u32; 3]) {
    memory::write_words(
        memory,
        address,
        &[
            40,
            2,
            u32::MAX,
            1 | u32::from(bits) << 16,
            3,
            u32::from(bits) / 4,
            0,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    memory::write_words(memory, address + 40, &masks).unwrap();
}

#[test]
fn bitfield_dibs_preserve_literal_565_and_swapped_32_bit_storage_and_dibsection_masks() {
    for (bits, masks, color, expected) in [
        (16, [0xf800, 0x07e0, 0x001f], 0x00ff00, vec![0xe0, 0x07]),
        (
            32,
            [0xff, 0xff00, 0xff0000],
            0x332211,
            vec![0x11, 0x22, 0x33, 0],
        ),
    ] {
        let (mut runtime, mut ram) = fixture();
        info(&mut ram, 0x10000, bits, masks);
        let bitmap = call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateDIBSection,
            &[0, 0x10000, 0, 0x10080, 0, 0],
        )
        .unwrap();
        let [pointer] = memory::words(&ram, 0x10080).unwrap();
        let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::SelectObject,
            &[dc, bitmap],
        )
        .unwrap();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::SetPixel,
                &[dc, 0, 0, color]
            ),
            Ok(color)
        );
        let mut raw = vec![0; expected.len()];
        ram.read(GuestAddress(pointer), &mut raw).unwrap();
        assert_eq!(raw, expected);
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetPixel, &[dc, 0, 0]),
            Ok(color)
        );
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GetObjectW,
                &[bitmap, 84, 0x10100]
            ),
            Ok(84)
        );
        let object = memory::words::<21>(&ram, 0x10100).unwrap();
        assert_eq!(object[10], 3);
        assert_eq!(&object[16..19], &masks);
    }
}

#[test]
fn bitfield_dib_transfer_converts_channels_and_preserves_requested_mask_metadata() {
    let (mut runtime, mut ram) = fixture();
    info(&mut ram, 0x10000, 16, [0xf800, 0x07e0, 0x001f]);
    let bitmap = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let [pointer] = memory::words(&ram, 0x10080).unwrap();
    ram.write(GuestAddress(pointer), &[0, 0xf8, 0xe0, 7])
        .unwrap();
    info(&mut ram, 0x10200, 32, [0xff, 0xff00, 0xff0000]);
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetDIBits,
            &[dc, bitmap, 0, 1, 0x10300, 0x10200, 0]
        ),
        Ok(1)
    );
    let mut bytes = [0; 8];
    ram.read(GuestAddress(0x10300), &mut bytes).unwrap();
    assert_eq!(bytes, [255, 0, 0, 0, 0, 255, 0, 0]);
    assert_eq!(
        memory::words::<3>(&ram, 0x10228).unwrap(),
        [0xff, 0xff00, 0xff0000]
    );
}

#[test]
fn invalid_bitfield_masks_fail_without_publishing_bitmap_output_or_backing() {
    for (bits, masks) in [
        (16, [0, 0x07e0, 0x001f]),
        (16, [0xf800, 0xf800, 0x001f]),
        (16, [0x5500, 0x00f0, 0x000f]),
        (16, [0x10000, 0x00f0, 0x000f]),
        (24, [0xff0000, 0xff00, 0xff]),
    ] {
        let (mut runtime, mut ram) = fixture();
        info(&mut ram, 0x10000, bits, masks);
        memory::write_words(&mut ram, 0x10080, &[0x12345678]).unwrap();
        let version = ram.transaction_version();
        let mapped = ram.mapped_pages();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::CreateDIBSection,
                &[0, 0x10000, 0, 0x10080, 0, 0]
            ),
            Err(GdiError32::InvalidRequest)
        );
        assert_eq!(runtime.model.object_count(), 0);
        assert_eq!(runtime.bytes, 0);
        assert_eq!(ram.transaction_version(), version);
        assert_eq!(ram.mapped_pages(), mapped);
        assert_eq!(memory::words::<1>(&ram, 0x10080).unwrap(), [0x12345678]);
    }
}

#[test]
fn extended_header_masks_survive_compatible_dib_and_header_only_query_boundary() {
    let (mut runtime, mut ram) = fixture();
    let mut header = [0u32; 27];
    header[..10].copy_from_slice(&[108, 2, u32::MAX, 1 | 16 << 16, 3, 4, 0, 0, 0, 0]);
    header[10..13].copy_from_slice(&[0xf800, 0x07e0, 0x001f]);
    memory::write_words(&mut ram, 0x10000, &header).unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let previous = call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    let compatible = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateCompatibleBitmap,
        &[dc, 3, 2],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetObjectW,
            &[compatible, 84, 0x10100]
        ),
        Ok(84)
    );
    let object = memory::words::<21>(&ram, 0x10100).unwrap();
    assert_eq!(object[10], 3);
    assert_eq!(&object[16..19], &[0xf800, 0x07e0, 0x001f]);
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, previous],
    )
    .unwrap();
    let query = 0x11fd8;
    memory::write_words(&mut ram, query, &[40, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetDIBits,
            &[dc, bitmap, 0, 0, 0, query, 0]
        ),
        Ok(1)
    );
    let header = memory::words::<10>(&ram, query).unwrap();
    assert_eq!(&header[..6], &[40, 2, 1, 1 | 16 << 16, 3, 4]);
    let version = ram.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetDIBits,
            &[dc, bitmap, 0, 0, 0, query, 0]
        )
        .is_err()
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(memory::words::<10>(&ram, query).unwrap(), header);
    ram.map_zeroed(
        crate::memory::PageRange::new(GuestAddress(0x12000), 1).unwrap(),
        crate::memory::Permissions::READ_WRITE,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetDIBits,
            &[dc, bitmap, 0, 0, 0, query, 0]
        ),
        Ok(1)
    );
    assert_eq!(
        memory::words::<3>(&ram, 0x12000).unwrap(),
        [0xf800, 0x07e0, 0x001f]
    );
}

#[test]
fn rgb_dibsection_metadata_matches_native_depth_specific_compression_and_masks() {
    // native windows run 38974156091 normalizes rgb555 metadata, while rgb888 keeps zero masks.
    for (bits, compression, masks) in [
        (16, 3, [0x7c00, 0x03e0, 0x001f]),
        (24, 0, [0; 3]),
        (32, 0, [0; 3]),
    ] {
        let (mut runtime, mut ram) = fixture();
        memory::write_words(
            &mut ram,
            0x10000,
            &[40, 2, u32::MAX, 1 | bits << 16, 0, 0, 0, 0, 0, 0],
        )
        .unwrap();
        let bitmap = call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateDIBSection,
            &[0, 0x10000, 0, 0x10080, 0, 0],
        )
        .unwrap();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GetObjectW,
                &[bitmap, 84, 0x10100]
            ),
            Ok(84)
        );
        let object = memory::words::<21>(&ram, 0x10100).unwrap();
        assert_eq!(object[10], compression, "BI_RGB depth {bits}");
        assert_eq!(&object[16..19], &masks, "BI_RGB depth {bits}");
    }
}
