use super::*;
use crate::process::{CallError, EngineInstance, HostError};

impl EngineInstance {
    pub fn configure_files(&mut self, config: FileConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.files.is_some() || self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::File(FileError32::Busy));
        }
        let epoch = crate::process::media::next_session()?;
        let mut runtime = FileRuntime32 {
            vfs: Vfs32::new(epoch, config.vfs).map_err(HostError::File)?,
            handles: KernelFileHandles32::new(config.handles).map_err(HostError::File)?,
            ordinary: OrdinaryRuntime32::new(config.finds, config.handles as usize)
                .map_err(HostError::File)?,
            mappings: MappingRuntime32::new(
                MappingLimits32 {
                    objects: config.mappings,
                    views: config.views,
                },
                config.view_bounds,
            )
            .map_err(HostError::File)?,
            asynchronous: AsyncRuntime32::new(config.operations, config.operation_bytes)
                .map_err(HostError::File)?,
            reply: None,
            events: Vec::new(),
            host_input: Default::default(),
        };
        for directory in ["C:\\Windows\\System32", "C:\\Temp"] {
            let path: Vec<u16> = directory.encode_utf16().collect();
            runtime
                .vfs
                .mount_directory(
                    &path,
                    crate::files::ATTR_DIRECTORY,
                    crate::files::FileTimes32::default(),
                )
                .map_err(HostError::File)?;
        }
        self.files = Some(runtime);
        Ok(())
    }
    pub fn files_epoch(&self) -> Result<u64, HostError> {
        self.memory()?;
        Ok(self
            .files
            .as_ref()
            .ok_or(HostError::File(FileError32::Unsupported))?
            .vfs
            .epoch())
    }
    pub(super) fn files_mut(&mut self) -> Result<&mut FileRuntime32, HostError> {
        self.memory()?;
        self.files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))
    }
    pub(in crate::process) fn files_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self
                .files
                .as_ref()
                .and_then(|runtime| runtime.reply)
                .is_some_and(|reply| reply.token == token)
    }
    pub fn pending_file_call(&self) -> Option<FileWait32> {
        let runtime = self.files.as_ref()?;
        let reply = runtime.reply?;
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(FileWait32 {
            epoch: runtime.vfs.epoch(),
            token: reply.token,
        })
    }
    pub fn resume_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .files
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.try_complete_file_call(reply.owner, token, reply.id, reply.frame)?;
        Ok(())
    }
    pub fn cancel_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .files
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, token)?;
        if let Some(request) = reply.request {
            match self.files.as_mut().unwrap().vfs.cancel_request(request) {
                Ok(()) | Err(FileError32::Stale) => {}
                Err(error) => return Err(HostError::File(error)),
            }
        }
        let runtime = self.files.as_mut().unwrap();
        runtime.mappings.cancel_pending();
        runtime.asynchronous.cancel_wait();
        runtime.reply = None;
        self.abandon_call(key, token)
    }
}
