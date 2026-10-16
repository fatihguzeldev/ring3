use super::*;

fn descriptor(aces: Vec<AccessAce32>) -> SecurityDescriptor32 {
    SecurityDescriptor32 {
        owner: Some(Sid32::system()),
        group: None,
        dacl: Dacl32::Acl(Acl32 { revision: 2, aces }),
        sacl: Dacl32::Absent,
        control: 0x8004,
        resource_manager: 0,
    }
}
fn ace(kind: u8, mask: u32) -> AccessAce32 {
    AccessAce32 {
        kind,
        flags: 0,
        mask,
        sid: Sid32::everyone(),
    }
}
#[test]
fn sid_and_relative_descriptor_roundtrip_preserve_owner_flags_and_ace_order() {
    let sid = Sid32::new(0x0102_0304_0506, &[0x1122_3344, u32::MAX]).unwrap();
    let mut sid_bytes = [0; 68];
    assert_eq!(sid.write(&mut sid_bytes).unwrap(), 16);
    assert_eq!(&sid_bytes[2..8], &[1, 2, 3, 4, 5, 6]);
    assert_eq!(Sid32::parse(&sid_bytes), Ok(sid));
    let mut sd = descriptor(vec![ace(0, 1), ace(1, 2)]);
    sd.owner = Some(sid);
    sd.control |= 0x1000;
    let bytes = sd.to_self_relative().unwrap();
    assert_eq!(
        SecurityDescriptor32::from_self_relative(&bytes).unwrap(),
        sd
    );
    let mut invalid = bytes.clone();
    invalid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        SecurityDescriptor32::from_self_relative(&invalid),
        Err(SecurityError32::InvalidDescriptor)
    );
    let acl = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut invalid = bytes;
    invalid[acl + 10..acl + 12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(
        SecurityDescriptor32::from_self_relative(&invalid),
        Err(SecurityError32::InvalidAcl)
    );
}
#[test]
fn access_checks_respect_ace_order_empty_null_generic_and_maximum_allowed() {
    let token = Token32::virtual_process();
    let mapping = FILE_GENERIC_MAPPING;
    let allow_then_deny = descriptor(vec![ace(0, 1), ace(1, 1)]);
    assert_eq!(allow_then_deny.check_access(&token, 1, mapping), Ok(1));
    let deny_then_allow = descriptor(vec![ace(1, 1), ace(0, 1)]);
    assert_eq!(
        deny_then_allow.check_access(&token, 1, mapping),
        Err(SecurityError32::AccessDenied)
    );
    let empty = descriptor(vec![]);
    assert_eq!(
        empty.check_access(&token, 1, mapping),
        Err(SecurityError32::AccessDenied)
    );
    let mut null = empty.clone();
    null.dacl = Dacl32::Null;
    assert_eq!(
        null.check_access(&token, 0x8000_0000, mapping),
        Ok(0x0012_0089)
    );
    let mixed = descriptor(vec![ace(1, 2), ace(0, 0x001f_01ff)]);
    assert_eq!(
        mixed.check_access(&token, 0x0200_0000, mapping),
        Ok(0x001f_01fd)
    );
    assert_eq!(
        mixed.check_access(&token, 0x0200_0002, mapping),
        Err(SecurityError32::AccessDenied)
    );
    assert_eq!(
        null.check_access(&token, 0x0100_0000, mapping),
        Err(SecurityError32::PrivilegeNotHeld)
    );
}
#[test]
fn owner_implicit_rights_can_be_restricted_by_owner_rights_sid() {
    let token = Token32::virtual_process();
    let mut sd = descriptor(vec![]);
    sd.owner = Some(token.user());
    assert_eq!(
        sd.check_access(&token, 0x0006_0000, FILE_GENERIC_MAPPING),
        Ok(0x0006_0000)
    );
    let Dacl32::Acl(acl) = &mut sd.dacl else {
        panic!()
    };
    acl.aces.push(AccessAce32 {
        kind: 0,
        flags: 0,
        mask: 0x0002_0000,
        sid: Sid32::owner_rights(),
    });
    assert_eq!(
        sd.check_access(&token, 0x0002_0000, FILE_GENERIC_MAPPING),
        Ok(0x0002_0000)
    );
    assert_eq!(
        sd.check_access(&token, 0x0004_0000, FILE_GENERIC_MAPPING),
        Err(SecurityError32::AccessDenied)
    );
}
#[test]
fn file_and_directory_inheritance_preserve_propagation_and_protected_empty_acl() {
    let token = Token32::virtual_process();
    let mut oi = ace(0, 1);
    oi.flags = 1;
    let mut ci = ace(1, 2);
    ci.flags = 2;
    let parent = descriptor(vec![oi, ci]);
    let file = SecurityDescriptor32::inherited(&parent, None, &token, false, FILE_GENERIC_MAPPING)
        .unwrap();
    let Dacl32::Acl(acl) = file.dacl else {
        panic!()
    };
    assert_eq!(acl.aces, vec![AccessAce32 { flags: 0x10, ..oi }]);
    let directory =
        SecurityDescriptor32::inherited(&parent, None, &token, true, FILE_GENERIC_MAPPING).unwrap();
    let Dacl32::Acl(acl) = directory.dacl else {
        panic!()
    };
    assert_eq!(acl.aces[0].flags, 0x19);
    assert_eq!(acl.aces[1].flags, 0x12);
    let mut protected = descriptor(vec![]);
    protected.owner = Some(token.user());
    protected.control |= 0x1000;
    let child = SecurityDescriptor32::inherited(
        &parent,
        Some(protected.clone()),
        &token,
        false,
        FILE_GENERIC_MAPPING,
    )
    .unwrap();
    assert_eq!(child.dacl, protected.dacl);
}
