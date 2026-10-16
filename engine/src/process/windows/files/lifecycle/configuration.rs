use super::super::*;
use crate::process::{EngineInstance, HostError};

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
            completion: crate::windows::files::completion::CompletionContext32::new(
                config.operations.min(config.handles as usize),
                config.handles as usize,
                config.operations,
                4096,
            )
            .map_err(HostError::File)?,
            completion_wait: None,
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
        if let Some(value) = self.process_launch_directory() {
            let directory = value.encode_utf16().collect::<Vec<_>>();
            runtime
                .vfs
                .mount_directory(
                    &directory,
                    crate::files::ATTR_DIRECTORY,
                    crate::files::FileTimes32::default(),
                )
                .map_err(HostError::File)?;
            let prepared = runtime
                .vfs
                .prepare_current_directory(&directory)
                .map_err(HostError::File)?;
            prepared.validate(&runtime.vfs).map_err(HostError::File)?;
            runtime.vfs.commit_namespace(prepared);
        }
        self.files = Some(runtime);
        Ok(())
    }
    pub fn files_epoch(&self) -> Result<u64, HostError> {
        Ok(self.files_storage()?.vfs.epoch())
    }
    pub(in crate::process::windows::files) fn files_mut(
        &mut self,
    ) -> Result<&mut FileRuntime32, HostError> {
        self.memory()?;
        self.files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))
    }
}
