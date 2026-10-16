use super::*;
use crate::windows::security::{AccessAce32, Acl32, Dacl32, SecurityDescriptor32, Token32};

fn explicit(token: &Token32, aces: Vec<AccessAce32>) -> SecurityDescriptor32 {
    SecurityDescriptor32 {
        owner: Some(token.user()),
        group: Some(token.primary_group()),
        dacl: Dacl32::Acl(Acl32 { revision: 2, aces }),
        sacl: Dacl32::Absent,
        control: 0x8004,
        resource_manager: 0,
    }
}
#[test]
fn new_explicit_empty_dacl_grants_creation_handle_but_reopen_is_denied_without_truncation() {
    let mut vfs = vfs();
    let token = Token32::virtual_process();
    let path = utf16("C:\\private");
    let receipt = vfs
        .prepare_open_with_security(
            &path,
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateNew,
                ..Default::default()
            },
            0x40000000,
            &token,
            Some(explicit(&token, vec![])),
        )
        .unwrap();
    assert_eq!(receipt.granted_access(), 0x00120116);
    let opened = vfs.commit_open(receipt);
    let FileIo32::Ready(write) = vfs.prepare_write(opened.open, b"retained").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_open_with_security(
            &path,
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateAlways,
                ..Default::default()
            },
            0x40000000,
            &token,
            None
        ),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(vfs.revision(), revision);
    assert_eq!(vfs.info(opened.file).unwrap().size, 8);
    let metadata = vfs
        .file_security(opened.file, 4, &token, Some(0x20000))
        .unwrap();
    assert_eq!(
        metadata.dacl,
        Dacl32::Acl(Acl32 {
            revision: 2,
            aces: vec![]
        })
    );
}
#[test]
fn changed_file_acl_preserves_existing_handle_grant_and_stable_identity_after_move() {
    let mut vfs = vfs();
    let token = Token32::virtual_process();
    let receipt = vfs
        .prepare_open_with_security(
            &utf16("C:\\old"),
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateNew,
                ..Default::default()
            },
            0x10000000,
            &token,
            None,
        )
        .unwrap();
    let opened = vfs.commit_open(receipt);
    let sd = explicit(
        &token,
        vec![AccessAce32 {
            kind: 1,
            flags: 0,
            mask: 0x001f01ff,
            sid: crate::windows::security::Sid32::everyone(),
        }],
    );
    let change = vfs
        .prepare_file_security(opened.file, 4, sd, &token, Some(0x40000))
        .unwrap();
    vfs.commit_file_security(change);
    let FileIo32::Ready(write) = vfs.prepare_write(opened.open, b"still authorized").unwrap()
    else {
        panic!()
    };
    vfs.commit_write(write);
    let moved = vfs
        .prepare_move(&utf16("C:\\old"), &utf16("C:\\new"), false)
        .unwrap();
    vfs.commit_namespace(moved);
    assert_eq!(vfs.path_info(&utf16("C:\\new")).unwrap().id, opened.file);
    assert!(matches!(
        vfs.prepare_open_with_security(
            &utf16("C:\\new"),
            FileOpenOptions32::default(),
            1,
            &token,
            None
        ),
        Err(FileError32::AccessDenied)
    ));
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_file_security(
            opened.file,
            4,
            explicit(&token, vec![]),
            &token,
            Some(0x20000)
        ),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(vfs.revision(), revision);
}
#[test]
fn prepared_acl_change_stale_receipt_and_denied_parent_creation_are_neutral() {
    let mut vfs = vfs();
    let token = Token32::virtual_process();
    let root = vfs.path_info(&utf16("C:\\")).unwrap().id;
    let change = vfs
        .prepare_file_security(root, 4, explicit(&token, vec![]), &token, Some(0x40000))
        .unwrap();
    let other = vfs
        .prepare_create_directory(&utf16("C:\\existing"), 0)
        .unwrap();
    vfs.commit_namespace(other);
    assert_eq!(change.validate(&vfs), Err(FileError32::Stale));
    let change = vfs
        .prepare_file_security(root, 4, explicit(&token, vec![]), &token, Some(0x40000))
        .unwrap();
    vfs.commit_file_security(change);
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_open_with_security(
            &utf16("C:\\denied"),
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateNew,
                ..Default::default()
            },
            1,
            &token,
            None
        ),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(vfs.revision(), revision);
    assert_eq!(
        vfs.path_info(&utf16("C:\\denied")),
        Err(FileError32::NotFound)
    );
}

#[test]
fn directory_security_propagates_in_order_and_respects_protection_sharing_and_stale_receipts() {
    let mut vfs = vfs();
    let token = Token32::virtual_process();
    for path in [
        "C:\\acl\\pass\\leaf",
        "C:\\acl\\protected\\leaf",
        "C:\\acl\\busy\\leaf",
    ] {
        vfs.mount_directory(&utf16(path), 0, FileTimes32::default())
            .unwrap();
    }
    let root = vfs.path_info(&utf16("C:\\acl")).unwrap().id;
    let protected = vfs.path_info(&utf16("C:\\acl\\protected")).unwrap().id;
    let protect = vfs
        .prepare_file_security(
            protected,
            0x80000000,
            explicit(&token, vec![]),
            &token,
            Some(0x40000),
        )
        .unwrap();
    vfs.commit_file_security(protect);
    let busy = open(
        &mut vfs,
        "C:\\acl\\busy",
        FileOpenOptions32 {
            access: FileAccess32::READ,
            share: FileShare32(0),
            directory: true,
            ..Default::default()
        },
    );
    let read_only = || {
        explicit(
            &token,
            vec![AccessAce32 {
                kind: 0,
                flags: 3,
                mask: 1,
                sid: crate::windows::security::Sid32::everyone(),
            }],
        )
    };
    let prepared = vfs
        .prepare_file_security_with_inheritance(root, 4, read_only(), &token, Some(0x40000), true)
        .unwrap();
    vfs.commit_file_security(prepared);
    for path in ["C:\\acl\\pass", "C:\\acl\\pass\\leaf"] {
        assert!(vfs.check_path_access(&utf16(path), 1, &token).is_ok());
        assert_eq!(
            vfs.check_path_access(&utf16(path), 2, &token),
            Err(FileError32::AccessDenied)
        );
        let file = vfs.path_info(&utf16(path)).unwrap().id;
        let descriptor = vfs.file_security(file, 4, &token, Some(0x20000)).unwrap();
        let Dacl32::Acl(acl) = descriptor.dacl else {
            panic!()
        };
        assert_eq!(acl.aces.len(), 1);
        assert_eq!(acl.aces[0].flags, 0x13);
    }
    for path in ["C:\\acl\\protected\\leaf", "C:\\acl\\busy\\leaf"] {
        assert!(vfs.check_path_access(&utf16(path), 2, &token).is_ok());
    }
    let leaf = vfs.path_info(&utf16("C:\\acl\\pass\\leaf")).unwrap().id;
    let clear = vfs
        .prepare_file_security(leaf, 4, explicit(&token, vec![]), &token, Some(0x40000))
        .unwrap();
    vfs.commit_file_security(clear);
    assert_eq!(
        vfs.check_path_access(&utf16("C:\\acl\\pass\\leaf"), 1, &token),
        Err(FileError32::AccessDenied)
    );
    let prepared = vfs
        .prepare_file_security_with_inheritance(root, 4, read_only(), &token, Some(0x40000), true)
        .unwrap();
    vfs.commit_file_security(prepared);
    assert!(
        vfs.check_path_access(&utf16("C:\\acl\\pass\\leaf"), 1, &token)
            .is_ok()
    );
    let prepared = vfs
        .prepare_file_security_with_inheritance(root, 4, read_only(), &token, Some(0x40000), true)
        .unwrap();
    vfs.close_open(busy.open).unwrap();
    assert_eq!(prepared.validate(&vfs), Err(FileError32::Stale));
    let prepared = vfs
        .prepare_file_security_with_inheritance(root, 4, read_only(), &token, Some(0x40000), false)
        .unwrap();
    vfs.commit_file_security(prepared);
    assert!(
        vfs.check_path_access(&utf16("C:\\acl\\busy\\leaf"), 2, &token)
            .is_ok()
    );
}
