use crate::memory::GuestAddress;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingAccess32 {
    Read,
    Write,
    Execute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingDomain32 {
    User,
}

/// an application access fault established by the current address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedPageFault32 {
    linear: GuestAddress,
    access: PagingAccess32,
    error_code: u32,
}

impl VerifiedPageFault32 {
    pub(crate) const fn application(
        linear: GuestAddress,
        access: PagingAccess32,
        present: bool,
    ) -> Self {
        Self {
            linear,
            access,
            error_code: present as u32
                | ((matches!(access, PagingAccess32::Write) as u32) << 1)
                | 4
                | ((matches!(access, PagingAccess32::Execute) as u32) << 4),
        }
    }

    pub const fn linear(self) -> GuestAddress {
        self.linear
    }
    pub const fn access(self) -> PagingAccess32 {
        self.access
    }
    pub const fn domain(self) -> PagingDomain32 {
        PagingDomain32::User
    }
    pub const fn error_code(self) -> u32 {
        self.error_code
    }
}

#[cfg(test)]
mod tests;
