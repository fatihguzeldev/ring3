use super::*;

fn headers(raw_payload: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0x200];
    bytes[..2].copy_from_slice(b"MZ");
    let half = |bytes: &mut [u8], offset, value: u16| {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes())
    };
    let word = |bytes: &mut [u8], offset, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
    };
    word(&mut bytes, 0x3c, 0x80);
    word(&mut bytes, 0x80, 0x4550);
    half(&mut bytes, 0x84, 0x14c);
    half(&mut bytes, 0x86, 2);
    half(&mut bytes, 0x94, 0xe0);
    half(&mut bytes, 0x98, 0x10b);
    word(&mut bytes, 0x98 + 56, 0x3000);
    word(&mut bytes, 0x98 + 60, 0x200);
    word(&mut bytes, 0x98 + 92, 16);
    word(&mut bytes, 0x98 + 112, 0x1000);
    word(&mut bytes, 0x98 + 116, 256);
    for (offset, rva, raw) in [(0x178, 0x1000, 0x200), (0x1a0, 0x2000, raw_payload)] {
        word(&mut bytes, offset + 8, 0x200);
        word(&mut bytes, offset + 12, rva);
        word(&mut bytes, offset + 16, 0x200);
        word(&mut bytes, offset + 20, raw);
    }
    bytes
}

#[test]
fn sparse_file_backing_reuses_the_resource_walker_without_mapping_or_reading_the_whole_pe() {
    let metadata = PeResourceFile32::parse(&headers(0x100000), 0x100200).unwrap();
    let mut resource = [0; 256];
    memory()
        .read(GuestAddress(BASE + 0x1000), &mut resource)
        .unwrap();
    let mut missing =
        PeResources32::from_file_parts(&metadata, &[], ParserLimits32::default()).unwrap();
    assert_eq!(
        missing.find_exact(
            &ResourceIdentifier32::Id(10),
            &ResourceIdentifier32::Id(7),
            0x409
        ),
        Err(ResourceError32::MissingFileRange {
            offset: 0x200,
            length: 16
        })
    );
    let parts = [ResourceFileSpan32 {
        offset: 0x200,
        bytes: resource.to_vec(),
    }];
    let mut reader =
        PeResources32::from_file_parts(&metadata, &parts, ParserLimits32::default()).unwrap();
    let location = reader
        .find_exact(
            &ResourceIdentifier32::Id(10),
            &ResourceIdentifier32::Id(7),
            0x409,
        )
        .unwrap();
    assert_eq!(
        reader.file_range(location).unwrap(),
        ResourceFileRange32 {
            offset: 0x100100,
            length: 3
        }
    );
    assert_eq!(
        reader.address(location.data_rva, location.size),
        Err(ResourceError32::Malformed)
    );
    assert_eq!(
        parts.iter().map(|part| part.bytes.len()).sum::<usize>(),
        256
    );
}

#[test]
fn physical_resource_offsets_above_four_gib_and_missing_header_ranges_remain_exact() {
    let metadata = PeResourceFile32::parse(&headers(0xffffff00), 0x1_0000_0100).unwrap();
    assert_eq!(
        metadata.range(0x2100, 3).unwrap(),
        ResourceFileRange32 {
            offset: 0x1_0000_0000,
            length: 3
        }
    );
    assert_eq!(metadata.range(0x2200, 1), Err(ResourceError32::Malformed));
    assert!(matches!(
        PeResourceFile32::parse(&headers(0x100000)[..0x100], 0x100200),
        Err(ResourceError32::MissingFileRange { .. })
    ));
    assert!(matches!(
        PeResourceFile32::parse(&headers(0x100000), 0x100100),
        Err(ResourceError32::Malformed)
    ));
}
