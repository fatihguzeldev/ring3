#![forbid(unsafe_code)]

use super::{CallError, EngineInstance, HostError};
use crate::{cpu::dbt::RegistryError, memory::GuestAddress};

pub(super) const RESIDENT_INSTALLATION_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidentInstallation {
    pub unit_id: u64,
    pub slot: u32,
}

impl EngineInstance {
    pub fn acknowledge_resident_installation(
        &mut self,
        key: u64,
        id: u64,
        slot: u32,
    ) -> Result<ResidentInstallation, HostError> {
        self.guard_resident_unit(key, id)?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.acknowledge_resident_slot(id, slot)
    }

    pub(super) fn acknowledge_resident_slot(
        &mut self,
        id: u64,
        slot: u32,
    ) -> Result<ResidentInstallation, HostError> {
        let existing = self
            .resident_installations
            .get(slot as usize)
            .ok_or(HostError::InvalidRequest)?;
        if existing.is_some_and(|installed| installed.unit_id != id)
            || self
                .resident_installations
                .iter()
                .flatten()
                .any(|installed| installed.unit_id == id && installed.slot != slot)
        {
            return Err(HostError::InvalidRequest);
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        if let Some(installed) = existing {
            return Ok(*installed);
        }
        let installed = ResidentInstallation { unit_id: id, slot };
        // occupied slots persist until explicit stale retirement or close.
        self.resident_installations[slot as usize] = Some(installed);
        Ok(installed)
    }

    pub fn lookup_installed_resident(
        &self,
        key: u64,
        pc: u32,
    ) -> Result<ResidentInstallation, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let unit_id = self.lookup_resident(pc)?.get();
        if self
            .callback
            .as_ref()
            .and_then(|callback| callback.authorized_resident_active_id())
            .is_some_and(|active_id| active_id != unit_id)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.resident_installations
            .iter()
            .flatten()
            .find(|installed| installed.unit_id == unit_id)
            .copied()
            .ok_or(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(pc),
            }))
    }
}
