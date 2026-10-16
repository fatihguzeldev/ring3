use crate::process::{
    EngineInstance, HostError,
    windows::resources::{ResourceError32 as E, types::ResourceCallOutcome32 as Outcome},
};
impl EngineInstance {
    pub(in crate::process) fn resume_resource_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<Outcome, HostError> {
        self.memory()?;
        let thread = self.current_windows_thread_id()?;
        let runtime = self
            .resources
            .as_ref()
            .ok_or(HostError::Resource(E::NotConfigured))?;
        let slot = runtime
            .pending
            .iter()
            .position(|pending| {
                pending.as_ref().is_some_and(|pending| {
                    pending.continuation == continuation && pending.thread == thread
                })
            })
            .ok_or(HostError::Resource(E::InvalidHandle))?;
        let pending = runtime.pending[slot].as_ref().unwrap();
        let live = self.resource_module_id(pending.module).is_ok();
        if let Some(scratch) = pending.scratch {
            scratch
                .check(self.memory.as_ref().unwrap())
                .map_err(HostError::Memory)?;
        }
        if result == 0 || pending.index + 1 == pending.items.len() || !live {
            if let Some(scratch) = pending.scratch {
                let versions = if live { 1 } else { 2 };
                self.memory
                    .as_ref()
                    .unwrap()
                    .ensure_transaction_versions(versions)
                    .map_err(HostError::Memory)?;
                scratch
                    .release(self.memory.as_mut().unwrap())
                    .map_err(HostError::Memory)?;
            }
            let runtime = self.resources.as_mut().unwrap();
            let pending = runtime.pending[slot].take().unwrap();
            runtime.enum_bytes -= pending.charged;
            return Ok(Outcome::Return {
                value: u32::from(result != 0 && live),
                last_error: (!live).then_some(6),
            });
        }
        self.memory
            .as_ref()
            .unwrap()
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let runtime = self.resources.as_mut().unwrap();
        let pending = runtime.pending[slot].as_mut().unwrap();
        let (request, scratch) = pending
            .request(
                self.memory.as_mut().unwrap(),
                runtime.config.scratch,
                pending.index + 1,
            )
            .map_err(HostError::Resource)?;
        if let Some(old) = pending.scratch {
            old.release(self.memory.as_mut().unwrap())
                .expect("checked callback scratch remains current");
        }
        pending.index += 1;
        pending.scratch = scratch;
        Ok(Outcome::Callback(request))
    }
}
