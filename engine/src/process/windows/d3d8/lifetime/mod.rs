mod bindings;
mod entry;
mod plan;
mod publication;

use crate::{memory::GuestAddress, process::windows::objects::PreparedGuestLifetime32};

#[derive(Default)]
pub(super) struct LifetimeChange32 {
    pub retain: Vec<GuestAddress>,
    pub release: Vec<GuestAddress>,
    pub hold: Vec<GuestAddress>,
    pub unhold: Vec<GuestAddress>,
}
pub(super) struct PreparedLifetimeChange32 {
    com: PreparedGuestLifetime32,
    external_releases: Vec<GuestAddress>,
    garbage: Vec<super::garbage::Garbage32>,
    locks: super::locks::PreparedLockDisposal32,
}
impl PreparedLifetimeChange32 {
    pub(super) fn destroyed(&self) -> &[crate::windows::com::ComIdentity32] {
        self.com.destroyed()
    }
}
pub(super) struct PreparedUpHolds32 {
    lifetime: PreparedLifetimeChange32,
    released: Vec<GuestAddress>,
}

#[cfg(test)]
mod tests;
