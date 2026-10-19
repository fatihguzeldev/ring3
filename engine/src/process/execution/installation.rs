#![forbid(unsafe_code)]

use crate::process::{CallError, EngineInstance, HostError};
use crate::{cpu::dbt::RegistryError, memory::GuestAddress};

pub(in crate::process) const RESIDENT_INSTALLATION_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidentInstallation {
    pub unit_id: u64,
    pub slot: u32,
}

#[cfg(test)]
mod tests;

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

    pub(in crate::process) fn acknowledge_resident_slot(
        &mut self,
        id: u64,
        slot: u32,
    ) -> Result<ResidentInstallation, HostError> {
        self.guard_owned_host_mutation()?;
        let existing = self
            .resident_installations
            .get(slot as usize)
            .ok_or(HostError::InvalidRequest)?;
        if existing.is_some_and(|installed| installed.unit_id != id)
            || self
                .resident_installations_by_id
                .binary_search_by_key(&id, |installed| installed.unit_id)
                .ok()
                .is_some_and(|position| self.resident_installations_by_id[position].slot != slot)
        {
            return Err(HostError::InvalidRequest);
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        if let Some(installed) = existing {
            return Ok(*installed);
        }
        let position = match self
            .resident_installations_by_id
            .binary_search_by_key(&id, |installed| installed.unit_id)
        {
            Ok(_) => return Err(HostError::InvalidRequest),
            Err(position) => position,
        };
        self.resident_installations_by_id
            .try_reserve(1)
            .map_err(|_| HostError::Resident(RegistryError::Allocation))?;
        let installed = ResidentInstallation { unit_id: id, slot };
        // occupied slots persist until explicit discard, stale retirement or close.
        self.resident_installations[slot as usize] = Some(installed);
        self.resident_installations_by_id
            .insert(position, installed);
        self.cache_track_installation(slot, id);
        Ok(installed)
    }

    pub(in crate::process) fn remove_resident_installation(&mut self, id: u64) {
        if let Ok(position) = self
            .resident_installations_by_id
            .binary_search_by_key(&id, |installed| installed.unit_id)
        {
            let installed = self.resident_installations_by_id.remove(position);
            if let Some(slot) = self.resident_installations.get_mut(installed.slot as usize)
                && *slot == Some(installed)
            {
                *slot = None;
            }
            self.cache_track_removal(id, Some(installed.slot));
        }
    }

    pub(in crate::process) fn resident_installation_by_id(
        &self,
        id: u64,
    ) -> Option<ResidentInstallation> {
        self.resident_installations_by_id
            .binary_search_by_key(&id, |installed| installed.unit_id)
            .ok()
            .map(|position| self.resident_installations_by_id[position])
            // the reverse index proposes a slot; its exact receipt remains authoritative.
            .filter(|installed| {
                self.resident_installations
                    .get(installed.slot as usize)
                    .copied()
                    .flatten()
                    == Some(*installed)
            })
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
        let installed = self
            .resident_installation_by_id(unit_id)
            .ok_or(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(pc),
            }))?;
        if let Some(cache) = self.execution_cache.as_ref() {
            cache.touch(installed.slot, installed.unit_id);
        }
        Ok(installed)
    }
}
