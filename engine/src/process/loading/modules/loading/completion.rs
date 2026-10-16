use super::super::{
    ModuleOperation32,
    initialization::{InitStep, LoadCompletion, ModuleOperation, OperationPurpose},
};
use crate::{
    loader::{ModuleError32, ModuleId32},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(super) fn begin_module_operation(
        &mut self,
        operation: ModuleOperation,
    ) -> Result<ModuleOperation32, HostError> {
        let result = self.advance_module_operation(operation);
        if result.is_err()
            && self
                .modules
                .operations
                .last()
                .is_some_and(|operation| operation.last_returned == 0 && operation.active.is_none())
        {
            let operation = self.modules.operations.pop().unwrap();
            operation.saved.restore(self.arena.as_mut().get_mut());
            self.pending_call = operation.outer;
            operation.execution.restore(self);
        }
        result
    }

    pub(in crate::process::loading::modules) fn prepare_module_rollback(
        &mut self,
        operation: &mut ModuleOperation,
        failed: Option<ModuleId32>,
    ) -> Result<(), HostError> {
        let OperationPurpose::Load {
            new_ids,
            initial,
            introduced_edges,
            ..
        } = &operation.purpose
        else {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        };
        let ids = self
            .modules
            .rollback_ids(new_ids, *initial, introduced_edges)
            .map_err(HostError::Module)?;
        let mut steps = self.detach_steps(&ids)?;
        if let Some(failed) =
            failed.filter(|failed| ids.contains(failed) && !self.modules.attached.contains(failed))
        {
            steps
                .try_reserve(2)
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            steps.push_front(InitStep::DllMain {
                module: failed,
                reason: 0,
                reserved: 0,
            });
            steps.push_front(InitStep::Tls {
                module: failed,
                reason: 0,
                index: 0,
            });
            self.modules.attached.push(failed);
        }
        let initial = *initial;
        let introduced_edges = match &mut operation.purpose {
            OperationPurpose::Load {
                introduced_edges, ..
            } => std::mem::take(introduced_edges),
            _ => unreachable!(),
        };
        operation.purpose = OperationPurpose::Rollback {
            ids,
            initial,
            introduced_edges,
        };
        operation.steps = steps;
        Ok(())
    }

    pub(in crate::process::loading::modules) fn complete_module_operation(
        &mut self,
        operation: ModuleOperation,
    ) -> Result<ModuleOperation32, HostError> {
        let result = (|| match &operation.purpose {
            OperationPurpose::Load {
                root,
                initial,
                completion,
                ..
            } => match completion {
                LoadCompletion::Library => {
                    self.modules
                        .retain(*root, *initial)
                        .map_err(HostError::Module)?;
                    let handle = self
                        .modules
                        .loaded
                        .get(*root)
                        .ok_or(HostError::Infrastructure)?
                        .base();
                    Ok(ModuleOperation32::Loaded { handle })
                }
                LoadCompletion::Export(address) => {
                    Ok(ModuleOperation32::ExportResolved { address: *address })
                }
            },
            OperationPurpose::Shutdown => {
                self.modules.attached.clear();
                Ok(ModuleOperation32::Released)
            }
            OperationPurpose::Unload { root, ids } => {
                self.commit_module_unload(ids, false, &[])?;
                if let Some(reference) = self
                    .modules
                    .references
                    .iter_mut()
                    .find(|reference| reference.id == *root)
                {
                    reference.explicit -= 1;
                }
                Ok(ModuleOperation32::Released)
            }
            OperationPurpose::Rollback {
                ids,
                initial,
                introduced_edges,
            } => {
                self.commit_module_unload(ids, *initial, introduced_edges)?;
                Ok(ModuleOperation32::InitializationFailed)
            }
        })();
        match result {
            Ok(result) => {
                if matches!(
                    operation.purpose,
                    OperationPurpose::Rollback { initial: true, .. }
                ) {
                    operation
                        .initial_before
                        .as_ref()
                        .ok_or(HostError::Infrastructure)?
                        .restore(self);
                    self.image = None;
                    self.image_started = false;
                } else {
                    operation.saved.restore(self.arena.as_mut().get_mut());
                }
                self.pending_call = operation.outer;
                operation.execution.restore(self);
                self.release_loader_thread();
                self.complete_module_windows_initializer(result, operation.last_returned)?;
                Ok(result)
            }
            Err(error) => {
                self.modules.operations.push(operation);
                Err(error)
            }
        }
    }
}
