use crate::{
    process::{EngineInstance, HostError},
    windows::shell::ShellFolderProfile32,
};
#[derive(Default)]
pub(in crate::process) struct ShellRuntime32 {
    pub(super) folders: ShellFolderProfile32,
}
impl EngineInstance {
    pub fn configure_shell_folders(
        &mut self,
        profile: ShellFolderProfile32,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.image_started
            || self.module_teb_address().is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        profile.validate().map_err(HostError::Shell)?;
        self.shell.folders = profile;
        Ok(())
    }
}

impl EngineInstance {
    pub fn configure_shell_folder(&mut self, kind: u32, path: Vec<u16>) -> Result<(), HostError> {
        let mut profile = self.shell.folders.clone();
        match kind {
            0 => profile.user_directory = path,
            1 => profile.default_user_directory = path,
            2 => profile.common_directory = path,
            3 => profile.program_files_directory = path,
            _ => return Err(HostError::InvalidRequest),
        }
        self.configure_shell_folders(profile)
    }
}
