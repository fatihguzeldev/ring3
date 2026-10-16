use super::*;
use crate::{memory::AddressSpace,process::{EngineInstance,windows::process::ProcessStartup32},windows::process::ProcessError32};
impl EngineInstance {
    pub(in crate::process) fn apply_launch_file_directory(&mut self,value:&str)->Result<(),crate::process::HostError> {
        if let Some(files)=self.files.as_mut() {
            let directory=value.encode_utf16().collect::<Vec<_>>();
            let prepared=files.vfs.prepare_current_directory(&directory).map_err(crate::process::HostError::File)?;
            prepared.validate(&files.vfs).map_err(crate::process::HostError::File)?;
            files.vfs.commit_namespace(prepared);
        }
        Ok(())
    }

    pub(in crate::process) fn configured_file_current_directory(&self)->Option<&[u16]> {
        self.files.as_ref().map(|files|files.vfs.current_directory())
    }
}
impl FileRuntime32 {
    pub(super) fn set_process_current_directory(&mut self,memory:&mut AddressSpace,startup:&mut ProcessStartup32,args:&[u32],wide:bool)->Result<FileCallOutcome32,FileError32> {
        let path=ordinary::memory::path(memory,args[0],wide)?;
        let absolute=self.vfs.full_path(&path)?;
        let prepared=self.vfs.prepare_current_directory(&absolute)?;
        prepared.validate(&self.vfs)?;
        startup.replace_current_directory(memory,&absolute).map_err(|error|match error {
            ProcessError32::Capacity=>FileError32::Capacity,
            ProcessError32::Allocation=>FileError32::Allocation,
            ProcessError32::NoAccess=>FileError32::NoAccess,
            _=>FileError32::InvalidParameter,
        })?;
        self.vfs.commit_namespace(prepared);
        Ok(FileCallOutcome32::success(1))
    }
}
