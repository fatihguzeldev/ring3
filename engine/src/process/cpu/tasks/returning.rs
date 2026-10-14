use super::TaskReturnAdmission;
use crate::{
    cpu::x86::tasks::{TaskAuthority, TaskDescriptorTable, TaskReturnError, prepare_return},
    memory::{GuestAddress, MemoryError},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn prepare_task_return(&self) -> Result<TaskReturnAdmission, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.prepare_task_return_bound()
    }

    pub fn prepare_task_return_resident(
        &self,
        key: u64,
        id: u64,
    ) -> Result<TaskReturnAdmission, HostError> {
        self.guard_material_resident(key, id)?;
        self.prepare_task_return_bound()
    }

    fn prepare_task_return_bound(&self) -> Result<TaskReturnAdmission, HostError> {
        let Some(sources) = self.task_return_sources else {
            return Ok(TaskReturnAdmission::MissingAuthority);
        };
        let current = sources.current;
        // the cached tss limit precedes any system-memory read, including an unmapped backlink.
        if current.limit() < 1 {
            return Ok(TaskReturnAdmission::InvalidTss(u32::from(
                current.selector() & !3,
            )));
        }
        let mut bytes = [0; 2];
        let memory = self.memory()?;
        if let Err(error) = memory.cpu_read(GuestAddress(current.base()), &mut bytes) {
            return Ok(match error {
                MemoryError::Fault(fault) => {
                    TaskReturnAdmission::NeedsPagingAuthority { fault, length: 2 }
                }
                error => TaskReturnAdmission::Infrastructure(error),
            });
        }
        let descriptors =
            TaskDescriptorTable::configured(sources.gdt_limit, &self.segment_descriptors);
        let authority = TaskAuthority::new(current, descriptors, &bytes);
        Ok(match prepare_return(Some(&authority)) {
            Ok(prepared) => TaskReturnAdmission::Admitted(prepared),
            Err(TaskReturnError::MissingAuthority) => TaskReturnAdmission::MissingAuthority,
            Err(TaskReturnError::InvalidTss(code)) => TaskReturnAdmission::InvalidTss(code),
            Err(TaskReturnError::NotPresent(code)) => TaskReturnAdmission::NotPresent(code),
        })
    }
}

#[cfg(test)]
#[path = "returning/tests.rs"]
mod tests;
