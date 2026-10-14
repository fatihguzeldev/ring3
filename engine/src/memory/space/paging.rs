use super::{
    AddressSpace, GUEST_PAGES, MemoryError, PAGE_SIZE, PageRange, cpu::CpuSpan, filled,
    page_address,
};
use crate::{
    cpu::x86::{LegacyPageViolation32, PagingAccess32, PagingDomain32},
    memory::{
        Access, GuestAddress, PagingCertificate32,
        paging::{PagingBacking32, PagingCheckError32, PagingFaultFacts32},
    },
};

impl AddressSpace {
    pub(crate) fn has_paging_certificates32(&self) -> bool {
        self.paging_certificates.is_some()
    }

    pub(crate) fn enable_paging_certificates32(&mut self) -> Result<(), MemoryError> {
        if self.paging_certificates.is_some() {
            return Err(MemoryError::InvalidRange);
        }
        self.paging_certificates = Some(filled(GUEST_PAGES, 0_u8)?.into_boxed_slice());
        Ok(())
    }

    pub(crate) fn certify_paging_range32(
        &mut self,
        range: PageRange,
        certificate: PagingCertificate32,
    ) -> Result<(), MemoryError> {
        let certificates = self
            .paging_certificates
            .as_ref()
            .ok_or(MemoryError::InvalidRange)?;
        if matches!(certificate, PagingCertificate32::Present { .. }) {
            self.require_mapped(range)?;
        }
        let bits = certificate.bits();
        if range.indices().all(|page| certificates[page] == bits) {
            return Ok(());
        }
        let version = self.advance_version()?;
        self.code_validation_version = version;
        for page in range.indices() {
            self.paging_certificates.as_mut().unwrap()[page] = bits;
            if self.mappings[page] != 0 {
                self.versions[self.mappings[page] as usize - 1].mapping = version;
            }
        }
        Ok(())
    }

    pub(super) fn invalidate_paging_certificate32(&mut self, page: usize) {
        if let Some(certificates) = self.paging_certificates.as_mut() {
            certificates[page] = 0;
        }
    }

    pub(crate) fn cpu_paging_check32(
        &self,
        address: GuestAddress,
        length: usize,
        access: PagingAccess32,
        domain: PagingDomain32,
        supervisor_write_protect: Option<bool>,
        backing: PagingBacking32,
    ) -> Result<(), PagingCheckError32> {
        let Some(certificates) = self.paging_certificates.as_ref() else {
            return Ok(());
        };
        let span = CpuSpan::new(address, length).map_err(PagingCheckError32::Memory)?;
        for part in span.parts() {
            if part.length == 0 {
                continue;
            }
            let first = (part.address.0 / PAGE_SIZE) as usize;
            let last = ((u64::from(part.address.0) + part.length as u64 - 1) / u64::from(PAGE_SIZE))
                as usize;
            for page in first..=last {
                let linear = GuestAddress(part.address.0.max(page_address(page).0));
                let certificate = PagingCertificate32::from_bits(certificates[page]);
                let violation = match certificate {
                    PagingCertificate32::Unknown => {
                        return Err(PagingCheckError32::Uncertified(linear));
                    }
                    PagingCertificate32::NotPresent => Some(LegacyPageViolation32::NotPresent),
                    PagingCertificate32::Present { user, writable } => {
                        if domain == PagingDomain32::User && !user {
                            Some(LegacyPageViolation32::SupervisorPage)
                        } else if access == PagingAccess32::Write && !writable {
                            if domain == PagingDomain32::User
                                || supervisor_write_protect == Some(true)
                            {
                                Some(LegacyPageViolation32::ReadOnlyPage)
                            } else if supervisor_write_protect.is_none() {
                                return Err(PagingCheckError32::UnknownWriteProtect(linear));
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                };
                if let Some(violation) = violation {
                    return Err(PagingCheckError32::Fault(PagingFaultFacts32 {
                        identity: self.identity,
                        linear,
                        access,
                        domain,
                        violation,
                    }));
                }
                // backing is a separate requirement: it never certifies or replaces paging facts.
                let checks = match backing {
                    PagingBacking32::Read => [Some(Access::Read), None],
                    PagingBacking32::Write => [Some(Access::Write), None],
                    PagingBacking32::Modify => [Some(Access::Read), Some(Access::Write)],
                    PagingBacking32::Execute => [Some(Access::Execute), None],
                };
                for host_access in checks.into_iter().flatten() {
                    self.access_slot(page, linear, host_access)
                        .map_err(PagingCheckError32::Memory)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
