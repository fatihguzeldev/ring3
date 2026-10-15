use super::{GuestComError32, GuestComObjects32};
#[cfg(test)]
use crate::{
    memory::WordWrite32,
    windows::com::{ComIdentity32, ReferenceState32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::com::ReferenceTransition32,
};

impl GuestComObjects32 {
    #[cfg(test)]
    pub(crate) fn state(&self, pointer: GuestAddress) -> Result<ReferenceState32, GuestComError32> {
        Ok(self.registry.state(pointer)?)
    }

    pub(crate) fn preview_reference_changes(
        &self,
        memory: &AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<Vec<ReferenceTransition32>, GuestComError32> {
        for &pointer in retain.iter().chain(release).chain(hold).chain(unhold) {
            self.preflight(memory, pointer)?;
        }
        Ok(self
            .registry
            .preview_reference_changes(retain, release, hold, unhold)?)
    }

    #[cfg(test)]
    pub(crate) fn apply_reference_changes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        self.apply_lifetime_changes(memory, retain, release, &[], &[])
    }

    #[cfg(test)]
    pub(crate) fn apply_lifetime_changes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        self.apply_lifetime_changes_with_writes(memory, retain, release, hold, unhold, &[])
    }

    #[cfg(test)]
    pub(crate) fn apply_lifetime_changes_with_writes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
        writes: &[WordWrite32],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        let prepared =
            self.prepare_lifetime_changes(memory, retain, release, hold, unhold, writes)?;
        self.commit_lifetime_changes(memory, prepared)
    }
}

#[cfg(test)]
mod tests;
