use super::{EngineInstance, HostError};
use ring3_observability::Observer;

impl EngineInstance {
    pub fn set_observer(&mut self, observer: Observer) -> Result<(), HostError> {
        self.memory()?;
        if self.observer.is_recording() || self.observer.stats().buffered_bytes != 0 {
            return Err(HostError::Call(super::CallError::Busy));
        }
        self.observer = observer;
        Ok(())
    }

    pub fn observer(&self) -> Observer {
        self.observer.clone()
    }
}
