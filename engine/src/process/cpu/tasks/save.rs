use crate::{
    cpu::x86::tasks::{MAX_TASK_SAVE_WORDS, TaskSave, TaskSavePlan},
    memory::{MemoryError, MemoryFault},
    process::{EngineInstance, HostError, StoreCompletion},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskSaveResult {
    MissingAuthority,
    InvalidTss(u32),
    NeedsPagingAuthority {
        fault: MemoryFault,
        length: u32,
    },
    Infrastructure(MemoryError),
    /// ram is published even if the subsequent material-currentness check returns an error.
    Saved {
        currentness: Result<StoreCompletion, HostError>,
    },
}

impl EngineInstance {
    pub(super) fn save_outgoing_task(
        &mut self,
        save: &TaskSave,
    ) -> Result<TaskSaveResult, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.save_outgoing_task_bound(None, save)
    }

    pub(super) fn save_outgoing_task_resident(
        &mut self,
        key: u64,
        id: u64,
        save: &TaskSave,
    ) -> Result<TaskSaveResult, HostError> {
        self.guard_material_resident(key, id)?;
        self.save_outgoing_task_bound(Some(id), save)
    }

    fn save_outgoing_task_bound(
        &mut self,
        resident: Option<u64>,
        save: &TaskSave,
    ) -> Result<TaskSaveResult, HostError> {
        let Some(sources) = self.task_return_sources else {
            return Ok(TaskSaveResult::MissingAuthority);
        };
        let current = sources.current;
        let plan = TaskSavePlan::new(current.format(), save);
        if current.limit() < plan.required_limit() {
            return Ok(TaskSaveResult::InvalidTss(u32::from(
                current.selector() & !3,
            )));
        }
        let mut addresses = [0; MAX_TASK_SAVE_WORDS];
        let mut values = [0; MAX_TASK_SAVE_WORDS];
        for (index, word) in plan.words().iter().enumerate() {
            addresses[index] = current.base().wrapping_add(u32::from(word.offset));
            values[index] = u32::from(word.value);
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        if let Err(error) = memory.cpu_write_stack(
            &addresses[..plan.words().len()],
            &values[..plan.words().len()],
            2,
        ) {
            return Ok(match error {
                MemoryError::Fault(fault) => {
                    TaskSaveResult::NeedsPagingAuthority { fault, length: 2 }
                }
                error => TaskSaveResult::Infrastructure(error),
            });
        }
        let changed = version != memory.code_validation_version();
        Ok(TaskSaveResult::Saved {
            currentness: self.cpu_store_currentness(changed, resident),
        })
    }
}

#[cfg(test)]
#[path = "save/tests.rs"]
mod tests;
