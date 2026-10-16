use super::returned;
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::files::tests::support::*,
    windows::files::FileApi32 as Api,
};
#[test]
fn mapped_query_preserves_view_allocation_copy_protection_and_split_regions() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 4, 0, 8192, 0],
    ));
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 1, 0, 0, 0],
    ));
    let initial = files.mappings.memory_info(&memory, view + 17).unwrap();
    assert_eq!(
        (
            initial.base,
            initial.allocation_base,
            initial.allocation_protection,
            initial.size,
            initial.state,
            initial.protection,
            initial.kind
        ),
        (view, view, 8, 8192, 0x1000, 8, 0x40000)
    );
    memory.write(GuestAddress(view), &[3]).unwrap();
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view)
            .unwrap()
            .protection,
        4
    );
    assert_eq!(
        files.mappings.memory_info(&memory, view).unwrap().size,
        4096
    );
    let second = PageRange::new(GuestAddress(view + 4096), 1).unwrap();
    memory.protect(second, Permissions::READ_WRITE).unwrap();
    files.mappings.record_protection(&memory, second, 4);
    assert_eq!(
        files.mappings.memory_info(&memory, view).unwrap().size,
        8192
    );
    let changed = files.mappings.memory_info(&memory, view + 4096).unwrap();
    assert_eq!(
        (
            changed.allocation_base,
            changed.allocation_protection,
            changed.protection,
            changed.kind
        ),
        (view, 8, 4, 0x40000)
    );
    memory.write(GuestAddress(view + 4096), &[9]).unwrap();
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view + 4096)
            .unwrap()
            .kind,
        0x40000
    );
    assert!(files.mappings.memory_info(&memory, view + 8192).is_none());
}
#[test]
fn mapped_query_reports_section_commitment_independent_of_noaccess_protection() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 0x04000004, 0, 8192, 0],
    ));
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 2, 0, 0, 0],
    ));
    assert_eq!(
        files.mappings.memory_info(&memory, view).unwrap().state,
        0x2000
    );
    files
        .try_commit_reserved_view(
            &mut memory,
            PageRange::new(GuestAddress(view), 1).unwrap(),
            Permissions::NONE,
        )
        .unwrap();
    let committed = files.mappings.memory_info(&memory, view).unwrap();
    assert_eq!(
        (committed.state, committed.protection, committed.size),
        (0x1000, 1, 4096)
    );
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view + 4096)
            .unwrap()
            .state,
        0x2000
    );
}

#[test]
fn detached_copy_reprotection_stays_copy_until_a_subsequent_private_write() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 4, 0, 4096, 0],
    ));
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 1, 0, 0, 0],
    ));
    let range = PageRange::new(GuestAddress(view), 1).unwrap();
    memory.write(GuestAddress(view), &[3]).unwrap();
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view)
            .unwrap()
            .protection,
        4
    );
    memory
        .protect_copy_with_word_write32(
            range,
            Permissions::READ_WRITE,
            crate::memory::WordWrite32 {
                address: GuestAddress(0x1200),
                value: 4,
            },
        )
        .unwrap();
    files.mappings.record_protection(&memory, range, 8);
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view)
            .unwrap()
            .protection,
        8
    );
    memory.write(GuestAddress(0x1300), &[9]).unwrap();
    assert_eq!(
        files
            .mappings
            .memory_info(&memory, view)
            .unwrap()
            .protection,
        8
    );
    memory.write(GuestAddress(view), &[5]).unwrap();
    let info = files.mappings.memory_info(&memory, view).unwrap();
    assert_eq!(
        (info.protection, info.kind, info.allocation_protection),
        (4, 0x40000, 8)
    );
}

#[test]
fn accepted_guard_clear_preserves_shared_and_copy_view_ownership_and_query_protection() {
    for access in [1, 2] {
        let (mut files, mut memory, _) = runtime();
        let section = returned(files.execute_mapping(
            &mut memory,
            &[],
            Api::CreateFileMappingW,
            &[u32::MAX, 0, 4, 0, 4096, 0],
        ));
        let view = returned(files.execute_mapping(
            &mut memory,
            &[],
            Api::MapViewOfFile,
            &[section, access, 0, 0, 0],
        ));
        let range = PageRange::new(GuestAddress(view), 1).unwrap();
        memory
            .protect(range, Permissions::READ_WRITE.with_guard())
            .unwrap();
        let raw = if access == 1 { 8 } else { 4 };
        files
            .mappings
            .record_protection(&memory, range, raw | 0x100);
        assert_eq!(
            files
                .mappings
                .memory_info(&memory, view)
                .unwrap()
                .protection,
            raw | 0x100
        );
        let guard = memory.guard_page(GuestAddress(view)).unwrap();
        memory.apply_private_pages(&[guard.clear()], &[]).unwrap();
        let info = files.mappings.memory_info(&memory, view).unwrap();
        assert_eq!(
            (info.allocation_base, info.protection, info.kind),
            (view, raw, 0x40000)
        );
        assert!(
            files
                .mappings
                .validate_view_protection(range, Permissions::READ_WRITE)
                .unwrap()
        );
        assert_eq!(
            returned(files.execute_mapping(&mut memory, &[], Api::UnmapViewOfFile, &[view])),
            1
        );
        assert!(files.mappings.memory_info(&memory, view).is_none());
    }
}
