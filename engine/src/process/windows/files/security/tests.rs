use super::super::{ordinary::FileCallOutcome32, tests::support};
use crate::{
    files::FileError32,
    memory::GuestAddress,
    windows::{
        files::FileApi32,
        security::{SecurityApi32, SecurityRuntime32},
    },
};

fn value(outcome: FileCallOutcome32) -> u32 {
    let FileCallOutcome32::Return { value, .. } = outcome else {
        panic!()
    };
    value
}
#[test]
fn create_file_absolute_empty_dacl_is_installed_and_existing_descriptor_pointer_is_ignored() {
    let (mut runtime, mut memory, _) = support::runtime();
    let security = SecurityRuntime32::default();
    support::path(&mut memory, 0x1200, "C:\\secured");
    for (address, word) in [
        (0x1400u32, 12u32),
        (0x1404, 0x1500),
        (0x1408, 1),
        (0x1510, 0x1600),
    ] {
        memory
            .write(GuestAddress(address), &word.to_le_bytes())
            .unwrap();
    }
    memory.write(GuestAddress(0x1500), &[1, 0, 4, 0]).unwrap();
    memory
        .write(GuestAddress(0x1600), &[2, 0, 8, 0, 0, 0, 0, 0])
        .unwrap();
    let handle = value(
        runtime
            .execute_ordinary_with_security(
                &mut memory,
                &security,
                FileApi32::CreateFileW,
                &[0x1200, 0x40000000, 7, 0x1400, 1, 0, 0],
            )
            .unwrap(),
    );
    assert_ne!(handle, u32::MAX);
    let info = runtime
        .execute_file_security(
            &mut memory,
            SecurityApi32::GetFileSecurityW,
            &[0x1200, 4, 0x1800, 128, 0x1700],
            security.token(),
        )
        .unwrap();
    assert_eq!(info, (1, None));
    assert_eq!(support::dword(&memory, 0x1700), 28);
    assert_eq!(support::dword(&memory, 0x1810), 20);
    assert!(matches!(
        runtime.execute_ordinary_with_security(
            &mut memory,
            &security,
            FileApi32::CreateFileW,
            &[0x1200, 0x40000000, 7, 0, 3, 0, 0]
        ),
        Err(FileError32::AccessDenied)
    ));
    memory
        .write(GuestAddress(0x1404), &u32::MAX.to_le_bytes())
        .unwrap();
    assert_ne!(
        value(
            runtime
                .execute_ordinary_with_security(
                    &mut memory,
                    &security,
                    FileApi32::CreateFileW,
                    &[0x1200, 0, 7, 0x1400, 3, 0, 0]
                )
                .unwrap()
        ),
        u32::MAX
    );
    memory
        .write(GuestAddress(0x1700), &0x11223344u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        runtime.execute_file_security(
            &mut memory,
            SecurityApi32::GetFileSecurityW,
            &[0x1200, 4, 0x6000, 128, 0x1700],
            security.token()
        ),
        Err(FileError32::NoAccess)
    );
    assert_eq!(support::dword(&memory, 0x1700), 0x11223344);
}

#[test]
fn pipe_security_info_uses_shared_descriptor_and_narrowed_handle_authority() {
    let (mut runtime, mut memory, mut sync) = support::runtime();
    let security = SecurityRuntime32::default();
    support::path(&mut memory, 0x1200, r"\\.\pipe\secure");
    let handle = value(
        runtime
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: security.token(),
                },
                FileApi32::CreateNamedPipeW,
                &[0x1200, 0x00040003, 0, 1, 16, 16, 0, 0],
            )
            .unwrap(),
    );
    let descriptor = runtime
        .query_file_security_info(
            &memory,
            false,
            false,
            &[handle, 1, 4, 0, 0, 0, 0, 0],
            security.token(),
        )
        .unwrap();
    assert!(matches!(
        descriptor.dacl,
        crate::windows::security::Dacl32::Acl(_)
    ));
    runtime
        .duplicate_kernel_handle(
            &mut memory,
            Some(&mut sync),
            &[u32::MAX, handle, u32::MAX, 0x1300, 0x20000, 0, 0],
            None,
        )
        .unwrap();
    let narrow = support::dword(&memory, 0x1300);
    memory
        .write(GuestAddress(0x1400), &[2, 0, 8, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(
        runtime.set_file_security_info(
            &memory,
            false,
            false,
            &[narrow, 1, 4, 0, 0, 0x1400, 0],
            security.token()
        ),
        Err(FileError32::AccessDenied)
    );
    runtime
        .set_file_security_info(
            &memory,
            false,
            false,
            &[handle, 1, 4, 0, 0, 0x1400, 0],
            security.token(),
        )
        .unwrap();
    let descriptor = runtime
        .query_file_security_info(
            &memory,
            true,
            true,
            &[0x1200, 1, 4, 0, 0, 0, 0, 0],
            security.token(),
        )
        .unwrap();
    let crate::windows::security::Dacl32::Acl(acl) = descriptor.dacl else {
        panic!("empty ACL retained")
    };
    assert!(acl.aces.is_empty());
}
