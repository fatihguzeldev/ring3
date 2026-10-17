use super::context::{Action32, ReferenceWork32, Stage32};
use crate::{
    process::windows::callbacks::WindowsCallbackProvider32,
    process::{CallError, EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, variants::Variant32},
};

impl EngineInstance {
    pub(in crate::process) fn automation_thread_temporary_allocations(
        &self,
        thread: u32,
    ) -> Result<Vec<(u32, u64)>, HostError> {
        self.automation_temporary_allocations(Some(thread))
    }
    pub(in crate::process) fn automation_process_temporary_allocations(
        &self,
    ) -> Result<Vec<(u32, u64)>, HostError> {
        self.automation_temporary_allocations(None)
    }
    fn automation_temporary_allocations(
        &self,
        thread: Option<u32>,
    ) -> Result<Vec<(u32, u64)>, HostError> {
        let mut allocations = Vec::new();
        for work in self
            .automation
            .references
            .iter()
            .filter(|work| thread.is_none_or(|thread| work.thread == thread))
        {
            if let Some(invoke) = &work.invoke
                && let Some(cleanup) = &invoke.cleanup
            {
                append_allocations(self, &mut allocations, cleanup.retirement_allocations())?;
            }
            if let Some(value) = self.accepted_automation_temporary(work)? {
                let plan = self.plan_owned_variant(Variant32::EMPTY, value, true)?;
                append_allocations(self, &mut allocations, plan.retirement_allocations())?;
            }
        }
        Ok(allocations)
    }
    fn accepted_automation_temporary(
        &self,
        work: &ReferenceWork32,
    ) -> Result<Option<Variant32>, HostError> {
        if let Stage32::InvokeReturned { returned, .. } = work.stage {
            return Ok(Some(returned));
        }
        let received = match work.stage {
            Stage32::Received {
                action: Action32::Invoke,
                ..
            } => true,
            Stage32::Calling {
                action: Action32::Invoke,
                ..
            } => self
                .windows_replies
                .iter()
                .find_map(|reply| {
                    reply.received_result(
                        WindowsCallbackProvider32::Automation,
                        work.owner,
                        work.token,
                        work.continuation,
                        work.frame,
                    )
                })
                .or_else(|| {
                    self.threads.windows_callback_result(
                        work.thread,
                        WindowsCallbackProvider32::Automation,
                        work.owner,
                        work.token,
                        work.continuation,
                        work.frame,
                    )
                })
                .is_some(),
            _ => false,
        };
        if received {
            let scratch = work
                .scratch
                .ok_or(HostError::Call(CallError::StateChanged))?;
            scratch.check(self.memory()?).map_err(HostError::Memory)?;
            self.read_automation_variant(scratch.address.0 + 32, false)
                .map(Some)
        } else {
            Ok(None)
        }
    }
    pub(in crate::process) fn commit_automation_temporary_retirement(
        &mut self,
        allocations: &[(u32, u64)],
    ) {
        let contains = |pointer| {
            allocations
                .binary_search_by_key(&pointer, |row| row.0)
                .is_ok()
        };
        let mut freed = 0;
        self.automation.strings.retain(|string| {
            if contains(string.pointer - 4) {
                freed += string.bytes;
                false
            } else {
                true
            }
        });
        self.automation.arrays.retain(|array| {
            if contains(array.base) {
                freed += array.allocation_bytes
                    + if array.separate_data {
                        array.layout.data_bytes
                    } else {
                        0
                    };
                false
            } else {
                true
            }
        });
        self.automation.bytes -= freed;
    }
}

fn append_allocations(
    engine: &EngineInstance,
    out: &mut Vec<(u32, u64)>,
    rows: &[(u32, u64)],
) -> Result<(), HostError> {
    out.try_reserve(rows.len())
        .map_err(|_| HostError::Automation(E::Allocation))?;
    for &(pointer, identity) in rows {
        if engine.windows_runtime_allocation_identity(pointer).ok() != Some(identity) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        match out.binary_search_by_key(&pointer, |row| row.0) {
            Ok(index) if out[index].1 != identity => {
                return Err(HostError::Call(CallError::StateChanged));
            }
            Ok(_) => {}
            Err(index) => out.insert(index, (pointer, identity)),
        }
    }
    Ok(())
}
