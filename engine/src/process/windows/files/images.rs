use super::*;
use crate::{
    files::{FileSection32, PreparedImageOpens32},
    process::{EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct ImageFile32 {
    source: ProviderFile32,
}
impl ImageFile32 {
    pub(in crate::process) fn is_source(self, source: ProviderFile32) -> bool {
        self.source == source
    }
}
pub(in crate::process) struct PreparedImageFiles32 {
    files: Vec<ImageFile32>,
    native: PreparedImageOpens32,
}
impl PreparedImageFiles32 {
    pub(in crate::process) fn files(&self) -> &[ImageFile32] {
        &self.files
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_image_files(
        &self,
        sources: &[(ProviderFile32, FileSection32)],
    ) -> Result<PreparedImageFiles32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let runtime = self.files_storage()?;
        let mut files = Vec::new();
        let mut opens = Vec::new();
        files
            .try_reserve_exact(sources.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        opens
            .try_reserve_exact(sources.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        for &(source, section) in sources {
            runtime
                .check_provider_file(source, thread)
                .map_err(HostError::File)?;
            files.push(ImageFile32 { source });
            opens.push((source.open, section));
        }
        let native = runtime
            .vfs
            .prepare_image_opens(&opens)
            .map_err(HostError::File)?;
        Ok(PreparedImageFiles32 { files, native })
    }
    pub(in crate::process) fn commit_image_files(&mut self, prepared: PreparedImageFiles32) {
        let files = self
            .files
            .as_mut()
            .expect("prepared image has a live filesystem");
        files.vfs.commit_image_opens(prepared.native);
        files
            .provider
            .leases
            .retain(|lease| !prepared.files.iter().any(|image| image.source == *lease));
    }
    pub(in crate::process) fn check_close_image_files(
        &self,
        images: &[ImageFile32],
    ) -> Result<(), HostError> {
        if images.is_empty() {
            return Ok(());
        }
        let files = self.files_storage()?;
        let mut opens = Vec::new();
        opens
            .try_reserve_exact(images.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        for image in images {
            if image.source.epoch != files.vfs.epoch() {
                return Err(HostError::File(FileError32::Stale));
            }
            files
                .vfs
                .open_info(image.source.open)
                .map_err(HostError::File)?;
            opens.push((image.source.open, 1));
        }
        files
            .vfs
            .check_close_open_group(&opens, 0, &[])
            .map_err(HostError::File)
    }
    pub(in crate::process) fn commit_close_image_files(&mut self, images: &[ImageFile32]) {
        if images.is_empty() {
            return;
        }
        let files = self.files.as_mut().expect("image close group was prepared");
        for image in images {
            files
                .vfs
                .close_open(image.source.open)
                .expect("image close group was checked before unmapping");
        }
    }
}
