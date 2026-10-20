use super::{ModuleProcess32, initialization::ModuleOperation};
use crate::process::{
    CallError, EngineInstance, HostError,
    calls::{
        call::PendingOwner,
        callback::{SuspendedCallback, SuspendedRecord},
    },
    windows::callbacks::WindowsReply32,
};

pub(super) struct ParkedModuleExecution32 {
    callback: Option<SuspendedCallback>,
    replies: Vec<WindowsReply32>,
    artifact: Option<crate::cpu::dbt::CompiledRegion>,
    generation: u32,
}
impl ParkedModuleExecution32 {
    pub(super) fn restore(self, engine: &mut EngineInstance) {
        assert!(engine.callback.is_none() && engine.windows_replies.is_empty());
        engine.revoke_owned_execution();
        engine.callback = self.callback;
        engine.windows_replies = self.replies;
        engine.artifact = self.artifact;
        engine.generation = self.generation;
    }
    fn execution_pins(&self) -> impl Iterator<Item = u64> + '_ {
        self.replies
            .iter()
            .filter_map(|reply| owner_pin(reply.owner()))
            .chain(
                std::iter::successors(self.callback.as_ref(), |callback| {
                    callback.parent.as_ref().map(|parent| &parent[0])
                })
                .flat_map(|callback| {
                    let mut pins = [None; 4];
                    pins[0] = owner_pin(callback.outer.owner);
                    if let SuspendedRecord::Resident {
                        record,
                        active_unit_id,
                        ..
                    } = callback.record
                    {
                        pins[1] = Some(record.outer_unit_id);
                        pins[2] = Some(record.callback_unit_id);
                        pins[3] = Some(active_unit_id);
                    }
                    pins.into_iter().flatten()
                }),
            )
    }
}
fn owner_pin(owner: PendingOwner) -> Option<u64> {
    match owner {
        PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => Some(id),
        PendingOwner::Replacement(_) | PendingOwner::Core(_) => None,
    }
}
pub(super) fn operation_execution_pins(
    operations: &[ModuleOperation],
) -> impl Iterator<Item = u64> + '_ {
    operations.iter().flat_map(|operation| {
        operation
            .outer
            .as_ref()
            .and_then(|outer| owner_pin(outer.owner))
            .into_iter()
            .chain(operation.execution.execution_pins())
    })
}
impl ModuleProcess32 {
    pub(in crate::process) fn parked_call(&self) -> Option<(PendingOwner, u32)> {
        self.operations
            .last()?
            .outer
            .as_ref()
            .map(|outer| (outer.owner, outer.token))
    }
    pub(in crate::process) fn has_parked_windows_reply(
        &self,
        provider: crate::process::windows::callbacks::WindowsCallbackProvider32,
    ) -> bool {
        self.operations.last().is_some_and(|operation| {
            operation.outer.as_ref().is_some_and(|outer| {
                operation.execution.replies.last().is_some_and(|reply| {
                    reply.provider() == provider && reply.matches_call(outer.owner, outer.token)
                })
            })
        })
    }
    pub(in crate::process) fn execution_pins(&self) -> impl Iterator<Item = u64> + '_ {
        operation_execution_pins(&self.operations)
    }
    pub(in crate::process) fn parked_callback_depth(&self) -> usize {
        self.operations
            .iter()
            .map(|operation| {
                operation
                    .execution
                    .callback
                    .as_ref()
                    .map_or(0, SuspendedCallback::depth)
            })
            .sum()
    }
    pub(in crate::process) fn parked_windows_reply_count(&self) -> usize {
        self.operations
            .iter()
            .map(|operation| operation.execution.replies.len())
            .sum()
    }
}
impl EngineInstance {
    pub(super) fn check_module_execution_admission(&self) -> Result<(), HostError> {
        if self.callback.is_some() {
            let pending = self
                .pending_call
                .as_ref()
                .ok_or(HostError::Call(CallError::Busy))?;
            self.validate_windows_provider_owner(pending.owner)?;
            self.checked_pending_call(pending.owner, pending.token)?;
        }
        Ok(())
    }
    pub(super) fn park_module_execution(&mut self) -> ParkedModuleExecution32 {
        ParkedModuleExecution32 {
            callback: self.callback.take(),
            replies: std::mem::take(&mut self.windows_replies),
            artifact: self.artifact.take(),
            generation: self.generation,
        }
    }
}

#[cfg(test)]
mod tests;
