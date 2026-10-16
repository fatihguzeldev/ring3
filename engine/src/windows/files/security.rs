use crate::{
    files::FileError32,
    windows::security::{Dacl32, FILE_GENERIC_MAPPING, SecurityDescriptor32, Token32},
};
fn check(
    descriptor: &SecurityDescriptor32,
    token: &Token32,
    access: u32,
    granted: Option<u32>,
) -> Result<(), FileError32> {
    if let Some(granted) = granted {
        if granted & access != access {
            return Err(FileError32::AccessDenied);
        }
    } else {
        descriptor.check_access(token, access, FILE_GENERIC_MAPPING)?;
    }
    Ok(())
}
pub(crate) fn query(
    descriptor: &SecurityDescriptor32,
    information: u32,
    token: &Token32,
    granted: Option<u32>,
) -> Result<SecurityDescriptor32, FileError32> {
    if information & !15 != 0 {
        return Err(FileError32::InvalidParameter);
    }
    if information & 7 != 0 {
        check(descriptor, token, 0x20000, granted)?;
    }
    if information & 8 != 0 {
        token.check_security_privilege()?;
        check(descriptor, token, 0x01000000, granted)?;
    }
    let mut descriptor = descriptor.snapshot()?;
    if information & 1 == 0 {
        descriptor.owner = None;
        descriptor.control &= !1;
    }
    if information & 2 == 0 {
        descriptor.group = None;
        descriptor.control &= !2;
    }
    if information & 4 == 0 {
        descriptor.dacl = Dacl32::Absent;
        descriptor.control &= !(4 | 8 | 0x1500);
    }
    if information & 8 == 0 {
        descriptor.sacl = Dacl32::Absent;
        descriptor.control &= !(16 | 32 | 0x2a00);
    }
    Ok(descriptor)
}
pub(crate) fn change(
    descriptor: &SecurityDescriptor32,
    information: u32,
    supplied: SecurityDescriptor32,
    token: &Token32,
    granted: Option<u32>,
) -> Result<SecurityDescriptor32, FileError32> {
    if information & !0xf000000f != 0
        || information & 0xa0000000 == 0xa0000000
        || information & 0x50000000 == 0x50000000
    {
        return Err(FileError32::InvalidParameter);
    }
    let mut required = 0;
    if information & 3 != 0 {
        required |= 0x80000;
    }
    if information & 0xa0000004 != 0 {
        required |= 0x40000;
    }
    if information & 0x50000008 != 0 {
        token.check_security_privilege()?;
        required |= 0x01000000;
    }
    check(descriptor, token, required, granted)?;
    let mut descriptor = descriptor.snapshot()?;
    if information & 1 != 0 {
        let owner = supplied.owner.ok_or(FileError32::InvalidOwner)?;
        if !token.may_set_owner(owner) {
            return Err(FileError32::InvalidOwner);
        }
        descriptor.owner = Some(owner);
        descriptor.control = (descriptor.control & !1) | (supplied.control & 1);
    }
    if information & 2 != 0 {
        let group = supplied.group.ok_or(FileError32::InvalidPrimaryGroup)?;
        if !token.matches(group, false) {
            return Err(FileError32::InvalidPrimaryGroup);
        }
        descriptor.group = Some(group);
        descriptor.control = (descriptor.control & !2) | (supplied.control & 2);
    }
    if information & 4 != 0 {
        descriptor.dacl = supplied.dacl;
        descriptor.control =
            (descriptor.control & !(4 | 8 | 0x500)) | (supplied.control & (4 | 8 | 0x500));
    }
    if information & 8 != 0 {
        descriptor.sacl = supplied.sacl;
        descriptor.control =
            (descriptor.control & !(16 | 32 | 0xa00)) | (supplied.control & (16 | 32 | 0xa00));
    }
    for (set, clear, flag) in [
        (0x80000000, 0x20000000, 0x1000),
        (0x40000000, 0x10000000, 0x2000),
    ] {
        if information & set != 0 {
            descriptor.control |= flag;
        }
        if information & clear != 0 {
            descriptor.control &= !flag;
        }
    }
    descriptor.to_self_relative()?;
    Ok(descriptor)
}
