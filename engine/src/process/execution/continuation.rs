//! explicit synchronous compiled execution: internal needcode seals can retain a
//! receipt witness instead of repeatedly copying structurally valid bank records.
//! this does not authorize host reentry or defer canonical x87 publication.
use crate::process::{EngineInstance, HostError};
#[cfg(any(target_arch = "wasm32", test))]
use {
    crate::process::{CallError, calls::call::PendingOwner},
    std::cell::Cell,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompiledExecutionUsage32 {
    pub seed_full_need_code_seals: u64,
    pub strict_full_need_code_seals: u64,
    pub light_need_code_seals: u64,
    pub full_materializations: u64,
    pub eligible_children: u64,
    pub strict_children: u64,
    pub continued_x87_step_attempts: u64,
    pub rejected_public_transitions: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub(in crate::process) struct CompiledExecutionStats32 {
    pub seed_full_need_code_seals: Cell<u64>,
    pub strict_full_need_code_seals: Cell<u64>,
    pub light_need_code_seals: Cell<u64>,
    pub full_materializations: Cell<u64>,
    pub eligible_children: Cell<u64>,
    pub strict_children: Cell<u64>,
    pub continued_x87_step_attempts: Cell<u64>,
    pub rejected_public_transitions: Cell<u64>,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(in crate::process) struct CompiledExecution32 {
    token: u64,
    seeded: Cell<bool>,
    eligible: Cell<bool>,
    continued: Cell<bool>,
}

impl EngineInstance {
    // a fallible admission barrier, deliberately separate from void revocation.
    // guest helpers run in active, so normal generated helper calls do not hit it.
    #[inline]
    pub(in crate::process) fn guard_compiled_public_transition(&self) -> Result<(), HostError> {
        #[cfg(any(target_arch = "wasm32", test))]
        if self.compiled_execution.is_some() && self.has_compiled_light_continuation() {
            let count = &self.compiled_execution_stats.rejected_public_transitions;
            count.set(count.get().saturating_add(1));
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl EngineInstance {
    pub(super) fn invalidate_compiled_context(&self) {
        if let Some(compiled) = &self.compiled_execution {
            compiled.seeded.set(false);
        }
    }

    pub(super) fn enable_compiled_execution(&mut self, token: u64) {
        self.compiled_execution = Some(CompiledExecution32 {
            token,
            seeded: Cell::new(false),
            eligible: Cell::new(false),
            continued: Cell::new(false),
        });
    }

    /// only an authorized, installed child reaches this after entry validation.
    /// unknown writers first materialize the previous seal and force a full exit.
    pub(in crate::process) fn prepare_compiled_child(
        &mut self,
        owner: PendingOwner,
    ) -> Result<Option<bool>, HostError> {
        let Some(compiled) = &self.compiled_execution else {
            return Ok(None);
        };
        if self.owned_receipt_token() != Some(compiled.token) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let id = match owner {
            PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => id,
            PendingOwner::Replacement(_) | PendingOwner::Core(_) => return Err(HostError::InvalidArtifact),
        };
        let eligible = self
            .resident
            .as_ref()
            .ok_or(HostError::InvalidArtifact)?
            .get_retained_raw(self.memory()?, id)
            .map_err(HostError::Resident)?
            .preserves_extended_record_validity();
        if !eligible {
            self.materialize_compiled_continuation()?;
        }
        Ok(Some(eligible))
    }

    pub(in crate::process) fn entered_compiled_child(&self, eligible: Option<bool>) {
        let Some(eligible) = eligible else {
            return;
        };
        let compiled = self
            .compiled_execution
            .as_ref()
            .expect("prepared compiled child");
        compiled.eligible.set(eligible);
        compiled.continued.set(eligible && compiled.seeded.get());
        // an unreviewed child cannot use a prior validity proof to skip its seal.
        if !eligible {
            compiled.seeded.set(false);
        }
        let count = if eligible {
            &self.compiled_execution_stats.eligible_children
        } else {
            &self.compiled_execution_stats.strict_children
        };
        count.set(count.get().saturating_add(1));
    }

    pub(in crate::process) fn compiled_need_code_token(&self, epoch: u64) -> Option<u64> {
        let compiled = self.compiled_execution.as_ref()?;
        if !compiled.seeded.get()
            || !compiled.eligible.get()
            || self.owned_receipt_token() != Some(compiled.token)
        {
            return None;
        }
        let (_, active, _) = self.owned_activation()?;
        (active == epoch).then_some(compiled.token)
    }

    pub(in crate::process) fn committed_compiled_need_code(&self, light: bool) {
        let Some(compiled) = &self.compiled_execution else {
            return;
        };
        let count = if light {
            &self.compiled_execution_stats.light_need_code_seals
        } else if compiled.eligible.get() {
            &self.compiled_execution_stats.seed_full_need_code_seals
        } else {
            &self.compiled_execution_stats.strict_full_need_code_seals
        };
        count.set(count.get().saturating_add(1));
        compiled.seeded.set(true);
    }

    pub(in crate::process) fn compiled_continuation_current(
        &self,
        token: u64,
        epoch: u64,
        owner: PendingOwner,
    ) -> bool {
        let Some(compiled) = &self.compiled_execution else {
            return false;
        };
        if compiled.token != token || self.owned_receipt_token() != Some(token) {
            return false;
        }
        let id = match owner {
            PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => id,
            PendingOwner::Replacement(_) | PendingOwner::Core(_) => return false,
        };
        let Some((last_epoch, installed)) =
            self.execution_frame.as_ref().and_then(|frame| frame.last)
        else {
            return false;
        };
        epoch == last_epoch
            && self.detection_epoch == epoch
            && installed.unit_id == id
            && self
                .resident_installations
                .get(installed.slot as usize)
                .copied()
                .flatten()
                == Some(installed)
    }

    #[inline]
    pub(in crate::process) fn record_compiled_x87_step_attempt(&self) {
        let Some(compiled) = &self.compiled_execution else {
            return;
        };
        // the enclosing scalar/pair/group entry already admitted the helper.
        // continued is established only at authorized child entry; this phase
        // check excludes calls after a sealed return without repeating authority.
        if compiled.continued.get()
            && matches!(
                self.exception_detection.as_ref().map(|slot| &slot[0]),
                Some(crate::process::cpu::events::detection::DetectionSlot32::Active { .. })
            )
        {
            let count = &self.compiled_execution_stats.continued_x87_step_attempts;
            count.set(count.get().saturating_add(1));
        }
    }

    pub fn compiled_execution_usage(
        &self,
        key: u64,
    ) -> Result<CompiledExecutionUsage32, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.execution_frame.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let stats = &self.compiled_execution_stats;
        Ok(CompiledExecutionUsage32 {
            seed_full_need_code_seals: stats.seed_full_need_code_seals.get(),
            strict_full_need_code_seals: stats.strict_full_need_code_seals.get(),
            light_need_code_seals: stats.light_need_code_seals.get(),
            full_materializations: stats.full_materializations.get(),
            eligible_children: stats.eligible_children.get(),
            strict_children: stats.strict_children.get(),
            continued_x87_step_attempts: stats.continued_x87_step_attempts.get(),
            rejected_public_transitions: stats.rejected_public_transitions.get(),
        })
    }
}

#[cfg(test)]
mod tests;
