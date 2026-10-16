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
