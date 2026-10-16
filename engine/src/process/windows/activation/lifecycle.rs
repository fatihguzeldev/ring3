use crate::{
    memory::PageRange,
    process::{EngineInstance, HostError},
    windows::activation::{
        ActivationError32 as E, PreparedActivationPush32, PreparedActivationThreadRetirement32,
    },
};

pub(in crate::process) struct PreparedActivationThreadExit32 {
    thread: u32,
    revision: u64,
    ranges: Vec<PageRange>,
    native: PreparedActivationThreadRetirement32,
}
impl PreparedActivationThreadExit32 {
    pub(in crate::process) fn ranges(&self) -> &[PageRange] {
        &self.ranges
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_activation_thread(
        &self,
        parent: u32,
        child: u32,
    ) -> Result<Option<PreparedActivationPush32>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let handle = runtime.native.current(parent);
        if handle != 0 {
            runtime
                .storage(handle)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
        }
        runtime
            .native
            .prepare_inherit(parent, child)
            .map_err(HostError::Activation)
    }
    pub(in crate::process) fn commit_activation_thread(
        &mut self,
        prepared: Option<PreparedActivationPush32>,
    ) {
        if let Some(prepared) = prepared {
            self.activation
                .as_mut()
                .expect("activation runtime was retained")
                .native
                .commit_activate(prepared);
        }
    }
    pub(in crate::process) fn prepare_activation_thread_exit(
        &self,
        thread: u32,
    ) -> Result<Option<PreparedActivationThreadExit32>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let native = runtime
            .native
            .prepare_thread_retirement(thread, Vec::new())
            .map_err(HostError::Activation)?;
        let ranges = runtime
            .disposal_ranges(self.memory()?, native.disposed())
            .map_err(HostError::Activation)?;
        Ok(Some(PreparedActivationThreadExit32 {
            thread,
            revision: runtime.native.revision(),
            ranges,
            native,
        }))
    }
    pub(in crate::process) fn check_activation_thread_exit(
        &self,
        prepared: &PreparedActivationThreadExit32,
    ) -> Result<(), HostError> {
        let runtime = self
            .activation
            .as_ref()
            .ok_or(HostError::Activation(E::Stale))?;
        if runtime.native.revision() != prepared.revision {
            return Err(HostError::Activation(E::Stale));
        }
        for &handle in prepared.native.disposed() {
            runtime
                .storage(handle)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
        }
        Ok(())
    }
    pub(in crate::process) fn commit_activation_thread_exit(
        &mut self,
        prepared: Option<PreparedActivationThreadExit32>,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        let runtime = self
            .activation
            .as_mut()
            .expect("activation thread exit was preflighted");
        assert_eq!(
            runtime.native.revision(),
            prepared.revision,
            "activation state remained exclusive during thread exit"
        );
        let disposed = runtime.native.commit_thread_retirement(prepared.native);
        runtime.retire_storage(&disposed);
        if runtime
            .reply
            .as_ref()
            .is_some_and(|reply| reply.thread == prepared.thread)
        {
            runtime.reply = None;
        }
    }
    pub(in crate::process) fn retire_activation_process(&mut self) {
        self.activation = None;
    }
}
