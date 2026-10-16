use super::*;
use crate::{process::{EngineInstance,HostError},windows::exceptions::ExceptionError32};
impl EngineInstance {
    /// Accept the terminal no-debugger disposition; retry the same delivery after supplying lazy file bytes.
    pub fn terminate_windows_exception(&mut self,key:u64,id:u64)->Result<(),HostError> {
        self.memory()?;
        if key!=self.key {return Err(HostError::InvalidArtifact);}
        let delivery=self.exceptions.deliveries.iter().rev().find(|delivery|delivery.thread==self.threads.current_id())
            .filter(|delivery|delivery.id==id&&delivery.phase==ExceptionPhase32::Unhandled)
            .ok_or(HostError::Exception(ExceptionError32::Stale))?;
        let code=delivery.record.code;
        self.publish_process_exit(code)
    }
}
impl ExceptionRuntime32 {
    pub(in crate::process) fn thread_unhandled(&self,thread:u32)->bool {
        self.deliveries.iter().rev().find(|delivery|delivery.thread==thread).is_some_and(|delivery|delivery.phase==ExceptionPhase32::Unhandled)
    }
}
