use super::FileEntry32;
use crate::{
    files::{FileError32, path::FilePathScope32},
    process::{EngineInstance, HostError, windows::commdlg::CapturedFileDialog32},
};

impl EngineInstance {
    pub(super) fn initial_common_dialog_directory(
        &self,
        request: &CapturedFileDialog32,
    ) -> Result<Vec<u16>, HostError> {
        let scope = self.capture_module_file_paths();
        if request
            .initial_file
            .iter()
            .any(|unit| matches!(*unit, 47 | 92))
            || request.initial_file.get(1) == Some(&58)
        {
            let path = scope
                .normalize(&request.initial_file)
                .map_err(HostError::File)?;
            if let Some(at) = path.iter().rposition(|&unit| unit == 92) {
                let directory = directory_prefix(&path, at);
                if self
                    .provider_path_info(directory)
                    .is_ok_and(|info| info.attributes & 0x10 != 0)
                {
                    return Ok(directory.to_vec());
                }
            }
        }
        if !request.initial_directory.is_empty() {
            let directory = scope
                .normalize(&request.initial_directory)
                .map_err(HostError::File)?;
            if self
                .provider_path_info(&directory)
                .is_ok_and(|info| info.attributes & 0x10 != 0)
            {
                return Ok(directory);
            }
        }
        if let Some(context) = &self.user32 {
            let recent = &context.file_dialogs.recent;
            if !recent.is_empty()
                && self
                    .provider_path_info(recent)
                    .is_ok_and(|info| info.attributes & 0x10 != 0)
            {
                return Ok(recent.clone());
            }
        }
        let directory = self
            .configured_file_current_directory()
            .ok_or(HostError::InvalidRequest)?
            .to_vec();
        self.provider_path_info(&directory)?;
        Ok(directory)
    }

    pub(super) fn common_dialog_entries(
        &self,
        directory: &[u16],
        request: &CapturedFileDialog32,
        index: u32,
    ) -> Result<Vec<FileEntry32>, HostError> {
        let mut pattern = directory.to_vec();
        if pattern.last() != Some(&92) {
            pattern.push(92);
        }
        pattern.push(42);
        let paths = match self.find_provider_paths(&pattern, 4096) {
            Ok(paths) => paths,
            Err(HostError::File(FileError32::NoMoreFiles)) => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(paths.len() + 1)
            .map_err(|_| HostError::Infrastructure)?;
        if directory.len() > 3 {
            let at = directory
                .iter()
                .rposition(|&unit| unit == 92)
                .ok_or(HostError::InvalidRequest)?;
            entries.push(FileEntry32 {
                path: directory_prefix(directory, at).to_vec(),
                name: vec![46, 46],
                directory: true,
            });
        }
        for path in paths {
            let info = self.provider_path_info(&path)?;
            let name = path[path
                .iter()
                .rposition(|&unit| unit == 92)
                .map_or(0, |at| at + 1)..]
                .to_vec();
            let is_directory = info.attributes & 0x10 != 0;
            if is_directory
                || request
                    .filters
                    .selected(index)
                    .is_none_or(|filter| filter.matches(&name))
            {
                entries.push(FileEntry32 {
                    path,
                    name,
                    directory: is_directory,
                });
            }
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(entries)
    }

    pub(super) fn common_dialog_selection(
        &mut self,
        token: u64,
        text: &[u16],
    ) -> Result<Vec<u16>, HostError> {
        let path = self.common_dialog_candidate(token, text)?;
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let plan = &context.file_dialogs.plans[context
            .file_dialogs
            .index(token)
            .map_err(HostError::User32)?];
        let admission = (
            plan.save,
            plan.request.input.flag(0x1000),
            plan.request.input.flag(0x10000),
        );
        let share_aware = plan.request.input.flag(0x4000);
        match self.admit_common_dialog_path(&path, admission.0, admission.1, admission.2) {
            Err(HostError::File(FileError32::SharingViolation)) if share_aware => {}
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        Ok(path)
    }
    pub(super) fn common_dialog_candidate(
        &mut self,
        token: u64,
        text: &[u16],
    ) -> Result<Vec<u16>, HostError> {
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let plan = &context.file_dialogs.plans[context
            .file_dialogs
            .index(token)
            .map_err(HostError::User32)?];
        let mut path = FilePathScope32::from_directory(plan.directory.clone())
            .normalize(text)
            .map_err(HostError::File)?;
        let file = path
            .iter()
            .rposition(|&unit| unit == 92)
            .map_or(0, |at| at + 1);
        if !plan.request.input.flag(0x100)
            && !path[file..].contains(&46)
            && !plan.request.default_extension.is_empty()
        {
            path.push(46);
            path.extend(plan.request.default_extension.iter().copied().take(3));
        }
        if plan.request.input.flag(0x800) || plan.request.input.flag(0x1000) {
            let at = path
                .iter()
                .rposition(|&unit| unit == 92)
                .ok_or(HostError::InvalidRequest)?;
            let info = self.provider_path_info(directory_prefix(&path, at))?;
            if info.attributes & 0x10 == 0 {
                return Err(HostError::File(FileError32::PathNotFound));
            }
        }
        if !plan.request.input.flag(0x100000)
            && plan.request.filters.selected(plan.filter_index).is_some()
            && crate::windows::text::ordinal_key(&path[file..])
                .ends_with(&".LNK".encode_utf16().collect::<Vec<_>>())
        {
            return Err(HostError::User32(
                crate::windows::user32::User32Error32::Win32(50),
            ));
        }
        if plan.request.input.flag(0x8000)
            && self
                .provider_path_info(&path)
                .is_ok_and(|info| info.attributes & 1 != 0)
        {
            return Err(HostError::File(FileError32::AccessDenied));
        }
        Ok(path)
    }
}

pub(super) fn directory_prefix(path: &[u16], slash: usize) -> &[u16] {
    &path[..if slash == 2 && path.get(1) == Some(&58) {
        3
    } else {
        slash
    }]
}
