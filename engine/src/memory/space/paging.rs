use super::{AddressSpace, PAGE_SIZE, cpu::CpuSpan, page_address};
use crate::memory::{
    Access, GuestAddress,
    paging::{PagingBacking32, PagingCheckError32, PagingFaultFacts32},
};

mod read;

impl AddressSpace {
    pub(crate) fn cpu_paging_check32(
        &self,
        address: GuestAddress,
        length: usize,
        access: Access,
        backing: PagingBacking32,
    ) -> Result<(), PagingCheckError32> {
        let span = CpuSpan::new(address, length).map_err(PagingCheckError32::Memory)?;
        let backing_access = match backing {
            PagingBacking32::Read => [Some(Access::Read), None],
            PagingBacking32::Write => [Some(Access::Write), None],
            PagingBacking32::Modify => [Some(Access::Read), Some(Access::Write)],
            PagingBacking32::Execute => [Some(Access::Execute), None],
        };
        for part in span.parts() {
            if part.length == 0 {
                continue;
            }
            let first = (part.address.0 / PAGE_SIZE) as usize;
            let last = ((u64::from(part.address.0) + part.length as u64 - 1) / u64::from(PAGE_SIZE))
                as usize;
            for page in first..=last {
                let linear = GuestAddress(part.address.0.max(page_address(page).0));
                let mapping = self.mappings[page];
                let present = mapping != 0;
                if !present
                    || !self.permissions[page].allows(access)
                    || backing_access
                        .into_iter()
                        .flatten()
                        .any(|required| !self.permissions[page].allows(required))
                {
                    return Err(PagingCheckError32::Fault(PagingFaultFacts32 {
                        identity: self.identity,
                        linear,
                        access,
                        present,
                    }));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
