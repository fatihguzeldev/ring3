use crate::{
    memory::PageRange,
    process::{EngineInstance, HostError},
    windows::activation::ActivationError32 as E,
};

pub(in crate::process) struct PreparedActivationHoldRelease32 {
    revision: Option<u64>,
    handles: Vec<u32>,
    disposed: Vec<u32>,
    ranges: Vec<PageRange>,
}
impl EngineInstance {
    pub(in crate::process) fn prepare_activation_hold_release(
        &self,
        handles: &[u32],
    ) -> Result<PreparedActivationHoldRelease32, HostError> {
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(handles.len())
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        owned.extend_from_slice(handles);
        let (revision, disposed, mut ranges) = if let Some(runtime) = &self.activation {
            let disposed = runtime
                .native
                .check_release_holds(handles, false)
                .map_err(HostError::Activation)?;
            let ranges = runtime
                .disposal_ranges(self.memory()?, &disposed)
                .map_err(HostError::Activation)?;
            (Some(runtime.native.revision()), disposed, ranges)
        } else {
            if handles.iter().any(|&handle| handle != 0) {
                return Err(HostError::Infrastructure);
            }
            (None, Vec::new(), Vec::new())
        };
        if !ranges.is_empty() {
            ranges.sort_unstable_by_key(|range| range.first);
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
        }
        Ok(PreparedActivationHoldRelease32 {
            revision,
            handles: owned,
            disposed,
            ranges,
        })
    }
    pub(in crate::process) fn commit_activation_hold_release(
        &mut self,
        prepared: PreparedActivationHoldRelease32,
    ) {
        if !prepared.ranges.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .unmap_ranges_with_word_writes(&prepared.ranges, &[])
                .expect("activation capture mappings were preflighted before retirement");
        }
        if let Some(revision) = prepared.revision {
            let runtime = self.activation.as_mut().unwrap();
            assert_eq!(runtime.native.revision(), revision);
            runtime.native.release_holds(&prepared.handles, false);
            runtime.retire_storage(&prepared.disposed);
        }
    }
}
