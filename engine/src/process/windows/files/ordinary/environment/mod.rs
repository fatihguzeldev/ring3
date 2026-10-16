use crate::{files::FileError32, process::windows::files::FileRuntime32};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEnvironment32 {
    pub application_directory: Vec<u16>,
    pub windows_directory: Vec<u16>,
    pub system_directory: Vec<u16>,
    pub temp_directory: Vec<u16>,
    pub search_path: Vec<Vec<u16>>,
}
impl Default for FileEnvironment32 {
    fn default() -> Self {
        Self {
            application_directory: "C:\\".encode_utf16().collect(),
            windows_directory: "C:\\Windows".encode_utf16().collect(),
            system_directory: "C:\\Windows\\System32".encode_utf16().collect(),
            temp_directory: "C:\\Temp".encode_utf16().collect(),
            search_path: Vec::new(),
        }
    }
}
impl FileRuntime32 {
    pub(in crate::process) fn virtual_system_directory(&self) -> &[u16] {
        &self.ordinary.environment.system_directory
    }

    pub(in crate::process) fn virtual_windows_directory(&self) -> &[u16] {
        &self.ordinary.environment.windows_directory
    }

    pub(in crate::process::windows::files) fn file_environment(&self) -> &FileEnvironment32 {
        &self.ordinary.environment
    }

    pub(in crate::process::windows::files) fn set_file_environment(
        &mut self,
        mut environment: FileEnvironment32,
    ) -> Result<(), FileError32> {
        if !self.ordinary.copies.is_empty() {
            return Err(FileError32::Busy);
        }
        if environment.search_path.len() > 256 {
            return Err(FileError32::InvalidLimits);
        }
        let mut total = 0usize;
        for path in [
            &mut environment.application_directory,
            &mut environment.windows_directory,
            &mut environment.system_directory,
            &mut environment.temp_directory,
        ]
        .into_iter()
        .chain(environment.search_path.iter_mut())
        {
            total = total.checked_add(path.len()).ok_or(FileError32::Capacity)?;
            if total > 524288
                || path.len() > 32767
                || path.len() < 3
                || path[1] != 58
                || !matches!(path[2], 47 | 92)
                || path.contains(&0)
            {
                return Err(FileError32::InvalidName);
            }
            *path = self.vfs.full_path(path)?;
        }
        self.ordinary.environment = environment;
        Ok(())
    }
}
