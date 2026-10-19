#[cfg(any(target_arch = "wasm32", test))]
use std::cell::Cell;

use crate::process::{EngineInstance, HostError};
#[cfg(any(target_arch = "wasm32", test))]
use crate::{
    cpu::x86::SegmentContext,
    process::{CallError, calls::call::PendingOwner, cpu::events::detection::DetectionSlot32},
};

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy)]
struct MaterialAuthority32 {
    epoch: u64,
    owner: u64,
    source: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(in crate::process) struct OwnedExecution32 {
    token: u64,
    memory: u64,
    thread: u32,
    callback: Option<u32>,
    material: Cell<Option<MaterialAuthority32>>,
    segments: Cell<Option<SegmentContext>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl OwnedExecution32 {
    fn invalidate(&self) {
        self.material.set(None);
        self.segments.set(None);
    }

    fn material_current(&self, epoch: u64, owner: u64, source: u64) -> bool {
        let Some(cached) = self.material.get() else {
            return false;
        };
        if cached.source != source {
            self.invalidate();
            return false;
        }
        cached.epoch == epoch && cached.owner == owner
    }
}

impl EngineInstance {
    pub(in crate::process) fn guard_owned_host_mutation(&self) -> Result<(), HostError> {
        #[cfg(any(target_arch = "wasm32", test))]
        if self.owned_execution.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub(in crate::process) fn invalidate_owned_context(&self) {
        #[cfg(any(target_arch = "wasm32", test))]
        if let Some(owned) = &self.owned_execution {
            owned.invalidate();
        }
    }

    pub(in crate::process) fn revoke_owned_execution(&mut self) {
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.owned_execution = None;
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) fn enable_owned_execution(&mut self, token: u64) {
        self.owned_execution = Some(OwnedExecution32 {
            token,
            memory: self.memory.as_ref().unwrap().identity(),
            thread: self.threads.current_id(),
            callback: self.callback.as_ref().map(|value| value.token()),
            material: Cell::new(None),
            segments: Cell::new(None),
        });
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn owned_activation(&self) -> Option<(&OwnedExecution32, u64, u64)> {
        let owned = self.owned_execution.as_ref()?;
        let frame = self.execution_frame.as_ref()?;
        if owned.token != frame.token
            || self.memory.as_ref()?.identity() != owned.memory
            || self.exit_code.is_some()
            || self.pending_call.is_some()
            || self.pending_windows_thread_schedule().is_some()
            || self.threads.current_id() != owned.thread
            || self.callback.as_ref().map(|value| value.token()) != owned.callback
        {
            return None;
        }
        let DetectionSlot32::Active {
            epoch,
            owner,
            callback_token,
            ..
        } = &self.exception_detection.as_ref()?[0]
        else {
            return None;
        };
        if *callback_token != owned.callback {
            return None;
        }
        let id = match *owner {
            PendingOwner::Resident(id) if owned.callback.is_none() => id,
            PendingOwner::ResidentCallback {
                unit_id,
                callback_token,
            } if Some(callback_token) == owned.callback
                && self.callback.as_ref()?.authorized_resident_active_id() == Some(unit_id) =>
            {
                unit_id
            }
            _ => return None,
        };
        let (last_epoch, installed) = frame.last?;
        if last_epoch != *epoch
            || installed.unit_id != id
            || self
                .resident_installations
                .get(installed.slot as usize)
                .copied()
                .flatten()
                != Some(installed)
        {
            return None;
        }
        Some((owned, *epoch, id))
    }

    #[cfg(test)]
    pub(in crate::process) fn cpu_owned_execution(&self, epoch: u64) -> u32 {
        u32::from(
            self.owned_activation()
                .is_some_and(|(_, active, _)| active == epoch),
        )
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn owned_material_current(&self, key: u64, id: u64) -> bool {
        if self.owned_execution.is_none() {
            return false;
        }
        if key != self.key {
            return false;
        }
        let Some((owned, epoch, owner)) = self.owned_activation() else {
            return false;
        };
        owner == id
            && owned.material_current(
                epoch,
                owner,
                self.memory.as_ref().unwrap().code_source_version(),
            )
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn remember_owned_material(&self, id: u64) {
        let Some((owned, epoch, owner)) = self.owned_activation() else {
            return;
        };
        if owner == id {
            owned.material.set(Some(MaterialAuthority32 {
                epoch,
                owner,
                source: self.memory.as_ref().unwrap().code_source_version(),
            }));
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn owned_operand_segment_context(
        &self,
    ) -> Option<Result<SegmentContext, HostError>> {
        let (owned, epoch, owner) = self.owned_activation()?;
        if !owned.material_current(
            epoch,
            owner,
            self.memory.as_ref().unwrap().code_source_version(),
        ) {
            return None;
        }
        if let Some(context) = owned.segments.get() {
            return Some(Ok(context));
        }
        let context = self.segment_context();
        if let Ok(context) = context {
            owned.segments.set(Some(context));
        }
        Some(context)
    }
}
