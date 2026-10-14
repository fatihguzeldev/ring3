use crate::memory::GuestAddress;

/// a caller-declared legacy32 paging diagnostic model, not engine paging configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageFaultProfile32 {
    supervisor_write_protect: Option<bool>,
}

impl PageFaultProfile32 {
    /// certifies pg1 with non-pae, non-pse paging and no nx/smep/smap/key/shadow/enclave extensions.
    pub const fn legacy32(supervisor_write_protect: Option<bool>) -> Self {
        Self {
            supervisor_write_protect,
        }
    }
    pub const fn supervisor_write_protect(self) -> Option<bool> {
        self.supervisor_write_protect
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingAccess32 {
    Read,
    Write,
    Execute,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingDomain32 {
    User,
    Supervisor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyPageViolation32 {
    NotPresent,
    SupervisorPage,
    ReadOnlyPage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPageFault32;

/// verified combinations of supplied architectural metadata; never inferred from a software memory fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedPageFault32 {
    profile: PageFaultProfile32,
    linear: GuestAddress,
    access: PagingAccess32,
    domain: PagingDomain32,
    violation: LegacyPageViolation32,
    error_code: u32,
}
impl VerifiedPageFault32 {
    pub fn new(
        profile: PageFaultProfile32,
        linear: GuestAddress,
        access: PagingAccess32,
        domain: PagingDomain32,
        violation: LegacyPageViolation32,
    ) -> Result<Self, InvalidPageFault32> {
        let valid = match violation {
            LegacyPageViolation32::NotPresent => true,
            LegacyPageViolation32::SupervisorPage => domain == PagingDomain32::User,
            LegacyPageViolation32::ReadOnlyPage => {
                access == PagingAccess32::Write
                    && (domain == PagingDomain32::User
                        || profile.supervisor_write_protect == Some(true))
            }
        };
        if !valid {
            return Err(InvalidPageFault32);
        }
        let error_code = u32::from(violation != LegacyPageViolation32::NotPresent)
            | (u32::from(access == PagingAccess32::Write) << 1)
            | (u32::from(domain == PagingDomain32::User) << 2);
        Ok(Self {
            profile,
            linear,
            access,
            domain,
            violation,
            error_code,
        })
    }
    pub const fn profile(self) -> PageFaultProfile32 {
        self.profile
    }
    pub const fn linear(self) -> GuestAddress {
        self.linear
    }
    pub const fn access(self) -> PagingAccess32 {
        self.access
    }
    pub const fn domain(self) -> PagingDomain32 {
        self.domain
    }
    pub const fn violation(self) -> LegacyPageViolation32 {
        self.violation
    }
    pub const fn error_code(self) -> u32 {
        self.error_code
    }
}

#[cfg(test)]
mod tests;
