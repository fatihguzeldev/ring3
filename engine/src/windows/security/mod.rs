mod access;
mod api;
mod descriptor;
mod sid;
mod token;

pub(crate) use access::{FILE_GENERIC_MAPPING, GenericMapping32};
pub(crate) use api::SecurityApi32;
#[cfg(test)]
pub(crate) use descriptor::Acl32;
pub(crate) use descriptor::{AccessAce32, Dacl32, SecurityDescriptor32};
pub(crate) use sid::Sid32;
pub(crate) use token::{SecurityRuntime32, Token32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SecurityError32 {
    InvalidSid,
    InvalidAcl,
    InvalidDescriptor,
    InvalidParameter,
    AccessDenied,
    PrivilegeNotHeld,
    Unsupported,
    Capacity,
    Allocation,
}
impl SecurityError32 {
    pub(crate) fn win32(self) -> u32 {
        match self {
            Self::InvalidSid => 1337,
            Self::InvalidAcl => 1336,
            Self::InvalidDescriptor => 1338,
            Self::InvalidParameter => 87,
            Self::AccessDenied => 5,
            Self::PrivilegeNotHeld => 1314,
            Self::Unsupported => 50,
            Self::Capacity | Self::Allocation => 8,
        }
    }
}

#[cfg(test)]
mod tests;
