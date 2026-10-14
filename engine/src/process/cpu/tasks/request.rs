#[cfg(any(target_arch = "wasm32", test))]
use crate::abi::{
    arena::TRANSFER_OFFSET,
    tasks::{TASK_RETURN_SOURCES_SIZE, decode_task_return_sources},
};
use crate::{
    abi::{arena::HELPER_OFFSET, memory_helper::HELPER_SIZE, tasks::encode_task_return_result},
    process::{EngineInstance, HostError, StoreCompletion},
};

impl EngineInstance {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn configure_task_return_sources_request(
        &mut self,
    ) -> Result<(), HostError> {
        self.task_return_configuration_guard()?;
        let (current, limit) = decode_task_return_sources(
            &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_RETURN_SOURCES_SIZE],
        )
        .map_err(|_| HostError::InvalidRequest)?;
        self.configure_task_return_sources(current, limit)
    }
}

impl EngineInstance {
    pub fn task_return(&mut self) -> Result<StoreCompletion, HostError> {
        let result = self.prepare_task_return()?;
        self.publish_task_return(result)
    }
    pub fn task_return_resident(
        &mut self,
        key: u64,
        id: u64,
    ) -> Result<StoreCompletion, HostError> {
        let result = self.prepare_task_return_resident(key, id)?;
        self.publish_task_return(result)
    }
    fn publish_task_return(
        &mut self,
        result: super::TaskReturnAdmission,
    ) -> Result<StoreCompletion, HostError> {
        let mut packet = [0; HELPER_SIZE];
        encode_task_return_result(
            self.task_return_sources
                .map(|sources| sources.current.base()),
            &result,
            &mut packet,
        )
        .map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        Ok(StoreCompletion::Complete)
    }
}

#[cfg(test)]
#[path = "request/tests.rs"]
mod tests;
