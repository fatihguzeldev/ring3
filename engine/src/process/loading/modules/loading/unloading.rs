use std::collections::VecDeque;

use super::super::{
    ModuleOperation32,
    initialization::{InitStep, ModuleOperation, OperationPurpose, SavedModuleContext},
};
use crate::{
    loader::{ModuleError32, ModuleId32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn free_library_module(&mut self, handle: u32) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let before = self.modules.loader_depth;
        self.acquire_loader_thread()?;
        let result = self.free_library_module_inner(handle);
        if !matches!(result, Ok(ModuleOperation32::Pending(_)))
            && self.modules.loader_depth > before
        {
            self.release_loader_thread();
        }
        result
    }

    fn free_library_module_inner(&mut self, handle: u32) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if !self.modules.operations.is_empty() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.check_module_execution_admission()?;
        let root = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        if self
            .modules
            .reference(root)
            .map_err(HostError::Module)?
            .pinned
        {
            return Ok(ModuleOperation32::Released);
        }
        let ids = self
            .modules
            .unreachable_after_release(root)
            .map_err(HostError::Module)?;
        if ids.is_empty() {
            self.modules
                .references
                .iter_mut()
                .find(|reference| reference.id == root)
                .unwrap()
                .explicit -= 1;
            return Ok(ModuleOperation32::Released);
        }
        let steps = self.detach_steps(&ids)?;
        let saved = SavedModuleContext::capture(self.arena())?;
        self.modules
            .operations
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let operation = ModuleOperation {
            purpose: OperationPurpose::Unload { root, ids },
            steps,
            active: None,
            saved,
            execution: self.park_module_execution(),
            outer: self.pending_call.take(),
            initial_before: None,
            last_returned: 0,
        };
        self.begin_module_operation(operation)
    }

    pub fn detach_modules_for_exit(&mut self) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let before = self.modules.loader_depth;
        self.acquire_loader_thread()?;
        let result = self.detach_modules_for_exit_inner();
        if !matches!(result, Ok(ModuleOperation32::Pending(_)))
            && self.modules.loader_depth > before
        {
            self.release_loader_thread();
        }
        result
    }

    fn detach_modules_for_exit_inner(&mut self) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        if !self.modules.operations.is_empty() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.check_module_execution_admission()?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(self.modules.attached.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        ids.extend_from_slice(&self.modules.attached);
        let mut steps = self.detach_steps(&ids)?;
        let reserved = self
            .modules
            .thread
            .as_ref()
            .map_or(1, |thread| thread.teb());
        for step in &mut steps {
            if let InitStep::DllMain {
                reserved: value, ..
            } = step
            {
                *value = reserved;
            }
        }
        self.modules
            .operations
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let operation = ModuleOperation {
            purpose: OperationPurpose::Shutdown,
            steps,
            active: None,
            saved: SavedModuleContext::capture(self.arena())?,
            execution: self.park_module_execution(),
            outer: self.pending_call.take(),
            initial_before: None,
            last_returned: 0,
        };
        self.begin_module_operation(operation)
    }

    pub(super) fn detach_steps(&self, ids: &[ModuleId32]) -> Result<VecDeque<InitStep>, HostError> {
        let mut steps = VecDeque::new();
        steps
            .try_reserve(ids.len() * 2)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for module in self
            .modules
            .attached
            .iter()
            .rev()
            .filter(|module| ids.contains(module))
        {
            steps.push_back(InitStep::Tls {
                module: *module,
                reason: 0,
                index: 0,
            });
            steps.push_back(InitStep::DllMain {
                module: *module,
                reason: 0,
                reserved: 0,
            });
        }
        Ok(steps)
    }

    pub(super) fn commit_module_unload(
        &mut self,
        ids: &[ModuleId32],
        initial: bool,
        introduced_edges: &[(ModuleId32, ModuleId32)],
    ) -> Result<(), HostError> {
        if ids.is_empty() && introduced_edges.is_empty() {
            return Ok(());
        }
        let mut plan = self
            .modules
            .loaded
            .plan_unload_rollback(self.memory()?, ids, introduced_edges)
            .map_err(HostError::Module)?;
        let thread = self
            .modules
            .thread
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        for allocation in thread
            .allocations
            .iter()
            .filter(|allocation| ids.contains(&allocation.module))
        {
            plan.add_range(allocation.range)
                .map_err(HostError::Module)?;
        }
        let mut writes = if initial {
            if let Some(range) = self.process_runtime.reservation() {
                plan.add_range(range).map_err(HostError::Module)?;
            }
            for range in self.windows_memory.committed_ranges() {
                plan.add_range(range).map_err(HostError::Module)?;
            }
            plan.add_range(
                PageRange::new(GuestAddress(thread.teb()), thread.runtime_pages())
                    .map_err(HostError::Memory)?,
            )
            .map_err(HostError::Module)?;
            plan.add_range(
                PageRange::new(
                    GuestAddress(thread.config.stack_base),
                    thread.config.stack_pages,
                )
                .map_err(HostError::Memory)?,
            )
            .map_err(HostError::Module)?;
            Vec::new()
        } else {
            thread.clear_writes(ids).map_err(HostError::Module)?
        };
        let secondary_tls = self.prepare_secondary_tls_unload(ids)?;
        for &range in secondary_tls.ranges() {
            plan.add_range(range).map_err(HostError::Module)?;
        }
        writes
            .try_reserve_exact(secondary_tls.words().len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        writes.extend_from_slice(secondary_tls.words());
        let loader_change = if initial {
            None
        } else {
            self.prepare_process_loader_change(&[], &[], ids)?
        };
        if let Some(change) = &loader_change {
            writes
                .try_reserve_exact(change.words().len())
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            writes.extend_from_slice(change.words());
        }
        writes.sort_unstable_by_key(|write| write.address.0);
        let empty_memory = initial
            .then(|| self.windows_memory.empty_same_limits())
            .transpose()
            .map_err(HostError::WindowsMemory)?;
        plan.unmap_with_writes(
            self.memory.as_mut().ok_or(HostError::Closed)?,
            &mut self.modules.loaded,
            &writes,
        )
        .map_err(HostError::Module)?;
        self.commit_secondary_static_tls(secondary_tls);
        self.commit_process_loader_change(loader_change);
        self.modules.remove_records(ids);
        if initial {
            self.modules.thread = None;
            self.threads.reset_primary_thread();
            self.process_runtime.startup = None;
            self.windows_memory =
                empty_memory.expect("initial rollback prepared empty Windows memory");
        }
        Ok(())
    }
}
