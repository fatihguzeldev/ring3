mod enumeration;
mod metadata;
mod propagation;

use super::{FileAccess32, FileError32, Vfs32, path};
use crate::windows::security::{
    FILE_GENERIC_MAPPING, SecurityDescriptor32, SecurityError32, Token32,
};

impl From<SecurityError32> for FileError32 {
    fn from(error: SecurityError32) -> Self {
        match error {
            SecurityError32::InvalidSid => Self::InvalidSid,
            SecurityError32::InvalidAcl => Self::InvalidAcl,
            SecurityError32::InvalidDescriptor => Self::InvalidSecurityDescriptor,
            SecurityError32::InvalidParameter => Self::InvalidParameter,
            SecurityError32::AccessDenied => Self::AccessDenied,
            SecurityError32::PrivilegeNotHeld => Self::PrivilegeNotHeld,
            SecurityError32::Unsupported => Self::Unsupported,
            SecurityError32::Capacity => Self::Capacity,
            SecurityError32::Allocation => Self::Allocation,
        }
    }
}
pub(super) fn windows_access(access: FileAccess32) -> u32 {
    let mut raw = 0;
    for (semantic, mask) in [
        (FileAccess32::READ, 1),
        (FileAccess32::WRITE, 2),
        (FileAccess32::APPEND, 4),
        (FileAccess32::EXECUTE, 0x20),
        (FileAccess32::DELETE, 0x10000),
        (FileAccess32::READ_ATTRIBUTES, 0x80),
        (FileAccess32::WRITE_ATTRIBUTES, 0x100),
        (FileAccess32::READ_CONTROL, 0x20000),
        (FileAccess32::WRITE_DAC, 0x40000),
        (FileAccess32::WRITE_OWNER, 0x80000),
    ] {
        if access.contains(semantic) {
            raw |= mask;
        }
    }
    raw
}
impl FileAccess32 {
    pub(crate) fn windows_access(self) -> u32 {
        windows_access(self)
    }
    pub(crate) fn from_windows(raw: u32) -> Self {
        let mut access = 0;
        for semantic in [
            Self::READ,
            Self::WRITE,
            Self::APPEND,
            Self::EXECUTE,
            Self::DELETE,
            Self::READ_ATTRIBUTES,
            Self::WRITE_ATTRIBUTES,
            Self::READ_CONTROL,
            Self::WRITE_DAC,
            Self::WRITE_OWNER,
        ] {
            if raw & windows_access(semantic) != 0 {
                access |= semantic.0;
            }
        }
        Self(access)
    }
}
impl Vfs32 {
    pub(crate) fn open_maximum_allowed(
        &self,
        open: super::OpenFileId,
    ) -> Result<bool, FileError32> {
        Ok(self.open(open)?.maximum_allowed)
    }
    pub(crate) fn check_path_access(
        &self,
        input: &[u16],
        desired: u32,
        token: &Token32,
    ) -> Result<(), FileError32> {
        let absolute = self.full_path(input)?;
        self.node(self.lookup(&absolute)?)?.security.check_access(
            token,
            desired,
            FILE_GENERIC_MAPPING,
        )?;
        Ok(())
    }
    pub(crate) fn check_parent_access(
        &self,
        input: &[u16],
        desired: u32,
        token: &Token32,
    ) -> Result<(), FileError32> {
        let absolute = self.full_path(input)?;
        self.check_path_access(path::parent(&absolute)?, desired, token)
    }
    pub(crate) fn check_delete_access(
        &self,
        input: &[u16],
        token: &Token32,
    ) -> Result<(), FileError32> {
        match self.check_path_access(input, 0x10000, token) {
            Err(FileError32::AccessDenied) => self.check_parent_access(input, 0x40, token),
            outcome => outcome,
        }
    }
    pub(super) fn inherited_file_security(
        &self,
        absolute: &[u16],
        explicit: Option<SecurityDescriptor32>,
        token: &Token32,
        directory: bool,
    ) -> Result<SecurityDescriptor32, FileError32> {
        let parent = self.node(self.lookup(path::parent(absolute)?)?)?;
        Ok(SecurityDescriptor32::inherited(
            &parent.security,
            explicit,
            token,
            directory,
            FILE_GENERIC_MAPPING,
        )?)
    }
}
