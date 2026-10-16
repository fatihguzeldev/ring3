use super::*;
fn fill(vfs: &mut Vfs32, request: FileReadRequest32) {
    let mut offset = 0;
    while offset < request.length {
        let count = (request.length - offset).min(4096);
        let bytes: Vec<u8> = (0..count)
            .map(|n| ((request.offset + u64::from(offset + n)) % 251) as u8)
            .collect();
        vfs.supply_bytes(request, offset, &bytes).unwrap();
        offset += count;
    }
}
#[test]
fn progressive_read_exceeds_single_page_cache_without_refetch_loop() {
    let mut vfs = Vfs32::new(
        7,
        FileLimits32 {
            cache_pages: 1,
            max_io_bytes: 4 * FILE_PAGE_BYTES,
            ..Default::default()
        },
    )
    .unwrap();
    let size = 3 * FILE_PAGE_BYTES + 17;
    let file = vfs
        .mount_asset(
            &utf16("C:\\asset"),
            AssetRef32 {
                id: 8,
                length: size as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let mut requests = 0;
    let read = loop {
        match vfs.prepare_read_at(file, 13, size - 13).unwrap() {
            FileIo32::Ready(read) => break read,
            FileIo32::NeedBytes(request) => {
                requests += 1;
                assert!(requests <= 4);
                fill(&mut vfs, request);
            }
        }
    };
    assert_eq!(read.bytes().len(), size - 13);
    assert!(
        read.bytes()
            .iter()
            .enumerate()
            .all(|(i, &b)| b == ((i + 13) % 251) as u8)
    );
    assert_eq!(requests, 4);
}
#[test]
fn supplied_original_asset_does_not_restore_a_truncated_then_regrown_tail() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\truncated"),
            AssetRef32 {
                id: 99,
                length: 100,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let shrink = vfs.prepare_resize(file, 3).unwrap();
    vfs.commit_resize(shrink);
    let grow = vfs.prepare_resize(file, 8).unwrap();
    vfs.commit_resize(grow);
    let FileIo32::NeedBytes(request) = vfs.prepare_read_at(file, 0, 8).unwrap() else {
        panic!()
    };
    assert!(!vfs.request_ready(request));
    vfs.supply_bytes(request, 0, &[42; 100]).unwrap();
    assert!(vfs.request_ready(request));
    let FileIo32::Ready(read) = vfs.prepare_read_at(file, 0, 8).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), &[42, 42, 42, 0, 0, 0, 0, 0]);
}
#[test]
fn supply_cancel_partial_overlay_and_shrink_grow_preserve_authority() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\asset"),
            AssetRef32 { id: 2, length: 100 },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let FileIo32::NeedBytes(request) = vfs.prepare_read_at(file, 0, 100).unwrap() else {
        panic!()
    };
    vfs.supply_bytes(request, 0, &[0, 1, 2]).unwrap();
    assert_eq!(
        vfs.supply_bytes(request, 1, &[9]),
        Err(FileError32::InvalidParameter)
    );
    vfs.cancel_request(request).unwrap();
    assert_eq!(vfs.supply_bytes(request, 0, &[0]), Err(FileError32::Stale));
    let write = loop {
        match vfs.prepare_write_at(file, 2, b"AB").unwrap() {
            FileIo32::Ready(write) => break write,
            FileIo32::NeedBytes(request) => fill(&mut vfs, request),
        }
    };
    vfs.commit_write(write);
    let FileIo32::Ready(read) = vfs.prepare_read_at(file, 0, 6).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), &[0, 1, 65, 66, 4, 5]);
    let resize = vfs.prepare_resize(file, 3).unwrap();
    vfs.commit_resize(resize);
    let resize = vfs.prepare_resize(file, 8).unwrap();
    vfs.commit_resize(resize);
    let FileIo32::Ready(read) = vfs.prepare_read_at(file, 0, 8).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), &[0, 1, 65, 0, 0, 0, 0, 0]);
}
#[test]
fn sparse_scatter_write_and_internal_retain_do_not_invalidate_resize() {
    let mut vfs = vfs();
    let opened = open(
        &mut vfs,
        "C:\\sparse",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let resize = vfs.prepare_resize(opened.file, 1u64 << 33).unwrap();
    vfs.retain_open(opened.open).unwrap();
    assert!(resize.validate(&vfs).is_ok());
    vfs.commit_resize(resize);
    let FileIo32::Ready(write) = vfs
        .prepare_writes_at(opened.file, &[(1, b"x"), (1u64 << 32, b"y")])
        .unwrap()
    else {
        panic!()
    };
    vfs.commit_write(write);
    assert_eq!(vfs.info(opened.file).unwrap().size, 1u64 << 33);
    assert_eq!(vfs.overlay_pages, 2);
}
#[test]
fn identical_async_reads_own_independent_fetch_and_cancel_receipts() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\concurrent"),
            AssetRef32 {
                id: 13,
                length: 200,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let FileIo32::NeedBytes(first) = vfs.prepare_read_at_for(1, file, 0, 200).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(second) = vfs.prepare_read_at_for(2, file, 0, 200).unwrap() else {
        panic!()
    };
    assert_ne!(first.request, second.request);
    vfs.cancel_request(first).unwrap();
    assert_eq!(vfs.supply_bytes(first, 0, &[0]), Err(FileError32::Stale));
    fill(&mut vfs, second);
    let FileIo32::Ready(read) = vfs.prepare_read_at_for(2, file, 0, 200).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes().len(), 200);
    assert!(vfs.pending_request().is_none());
}
#[test]
fn overlay_capacity_is_revalidated_after_a_pending_asset_write() {
    let mut vfs = Vfs32::new(
        1,
        FileLimits32 {
            overlay_pages: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let file = vfs
        .mount_asset(
            &utf16("C:\\asset"),
            AssetRef32 { id: 14, length: 4 },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let FileIo32::NeedBytes(request) = vfs.prepare_write_at(file, 0, b"a").unwrap() else {
        panic!()
    };
    let other = open(
        &mut vfs,
        "C:\\other",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(other.open, b"b").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    fill(&mut vfs, request);
    assert!(matches!(
        vfs.prepare_write_at(file, 0, b"a"),
        Err(FileError32::DiskFull)
    ));
    assert_eq!(vfs.info(file).unwrap().size, 4);
    assert_eq!(vfs.overlay_pages, 1);
}
#[test]
fn stale_supply_sparse_zero_write_and_export_ack_are_failure_neutral() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\asset"),
            AssetRef32 { id: 15, length: 20 },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let FileIo32::NeedBytes(request) = vfs.prepare_read_at(file, 0, 20).unwrap() else {
        panic!()
    };
    let resize = vfs.prepare_resize(file, 0).unwrap();
    vfs.commit_resize(resize);
    assert_eq!(vfs.supply_bytes(request, 0, &[0]), Err(FileError32::Stale));
    let FileIo32::Ready(write) = vfs.prepare_write_at(file, 1u64 << 32, b"").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    assert_eq!(vfs.info(file).unwrap().size, 0);
    let revision = vfs.revision();
    assert_eq!(
        vfs.acknowledge_overlay(2, revision),
        Err(FileError32::Stale)
    );
    assert!(vfs.overlay_entry(0).unwrap().is_some());
    vfs.acknowledge_overlay(1, revision).unwrap();
    assert!(vfs.overlay_entry(0).unwrap().is_none());
}
#[test]
fn large_immutable_asset_small_reads_and_operation_cleanup_are_bounded() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\large"),
            AssetRef32 {
                id: 16,
                length: 1u64 << 33,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let FileIo32::NeedBytes(first) = vfs.prepare_read_at_for(11, file, 0, 4).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(second) = vfs.prepare_read_at_for(12, file, 1u64 << 32, 4).unwrap()
    else {
        panic!()
    };
    vfs.cancel_operation_io(11);
    assert_eq!(vfs.supply_bytes(first, 0, &[0]), Err(FileError32::Stale));
    fill(&mut vfs, second);
    let FileIo32::Ready(read) = vfs.prepare_read_at_for(12, file, 1u64 << 32, 4).unwrap() else {
        panic!()
    };
    assert_eq!(
        read.bytes(),
        &(0..4)
            .map(|n| (((1u64 << 32) + n) % 251) as u8)
            .collect::<Vec<_>>()
    );
    assert!(vfs.cache.stages.is_empty());
}

#[test]
fn guest_thread_async_and_host_fetch_domains_do_not_share_cancel_authority() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\thread-source"),
            AssetRef32 {
                id: 19,
                length: 200,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    vfs.set_io_thread(1);
    let FileIo32::NeedBytes(first) = vfs.prepare_read_at(file, 0, 200).unwrap() else {
        panic!()
    };
    vfs.set_io_thread(2);
    let FileIo32::NeedBytes(second) = vfs.prepare_read_at(file, 0, 200).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(asynchronous) = vfs.prepare_read_at_for(1, file, 0, 200).unwrap()
    else {
        panic!()
    };
    assert_ne!(first.request, second.request);
    assert_ne!(first.request, asynchronous.request);
    vfs.cancel_guest_thread_io(1);
    assert_eq!(vfs.supply_bytes(first, 0, &[1]), Err(FileError32::Stale));
    fill(&mut vfs, second);
    fill(&mut vfs, asynchronous);
    assert!(matches!(
        vfs.prepare_read_at(file, 0, 200).unwrap(),
        FileIo32::Ready(_)
    ));
    assert!(matches!(
        vfs.prepare_read_at_for(1, file, 0, 200).unwrap(),
        FileIo32::Ready(_)
    ));
}

#[test]
fn supplied_thread_progress_survives_single_page_cache_eviction_without_refetching() {
    let mut vfs = Vfs32::new(
        7,
        FileLimits32 {
            cache_pages: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let first = vfs
        .mount_asset(
            &utf16("C:\\first-thread"),
            AssetRef32 {
                id: 81,
                length: 200,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let second = vfs
        .mount_asset(
            &utf16("C:\\second-thread"),
            AssetRef32 {
                id: 82,
                length: 200,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    vfs.set_io_thread(1);
    let FileIo32::NeedBytes(one) = vfs.prepare_read_at(first, 0, 200).unwrap() else {
        panic!()
    };
    fill(&mut vfs, one);
    vfs.set_io_thread(2);
    let FileIo32::NeedBytes(two) = vfs.prepare_read_at(second, 0, 200).unwrap() else {
        panic!()
    };
    fill(&mut vfs, two);
    vfs.set_io_thread(1);
    let FileIo32::Ready(one) = vfs.prepare_read_at(first, 0, 200).unwrap() else {
        panic!("host-accepted bytes were already staged")
    };
    assert_eq!(one.bytes().len(), 200);
    vfs.set_io_thread(2);
    assert!(matches!(
        vfs.prepare_read_at(second, 0, 200).unwrap(),
        FileIo32::Ready(_)
    ));
    assert!(vfs.pending_request().is_none());
}
