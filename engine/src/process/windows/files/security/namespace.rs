use super::super::{FileCallOutcome32, FileRuntime32, ordinary::memory};
use crate::{
    files::{ATTR_DIRECTORY, FileError32},
    memory::AddressSpace,
    windows::{files::FileApi32 as Api, security::Token32},
};

impl FileRuntime32 {
    pub(in crate::process) fn set_security_ansi(&mut self, ansi: crate::windows::text::CodePage32) {
        self.ordinary.ansi = ansi;
    }
    pub(in crate::process::windows::files) fn create_directory_with_security(
        &mut self,
        memory_space: &AddressSpace,
        api: Api,
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let extended = matches!(api, Api::CreateDirectoryExA | Api::CreateDirectoryExW);
        let path = memory::path(
            memory_space,
            args[usize::from(extended)],
            api.name().ends_with('W'),
            self.ordinary.ansi,
        )?;
        let attributes_pointer = args[if extended { 2 } else { 1 }];
        memory::inherit(memory_space, attributes_pointer)?;
        let descriptor = crate::process::windows::security::memory::attributes_descriptor(
            memory_space,
            attributes_pointer,
        )?;
        let attributes = if extended {
            let template = memory::path(
                memory_space,
                args[0],
                api.name().ends_with('W'),
                self.ordinary.ansi,
            )?;
            self.vfs.check_path_access(&template, 0x80, token)?;
            let info = self.vfs.path_info(&template)?;
            if info.attributes & ATTR_DIRECTORY == 0 {
                return Err(FileError32::NotDirectory);
            }
            info.attributes
        } else {
            0
        };
        let prepared = self
            .vfs
            .prepare_create_directory_with_security(&path, attributes, token, descriptor)?;
        self.vfs.commit_namespace(prepared);
        Ok(FileCallOutcome32::Return {
            value: 1,
            last_error: None,
        })
    }
    pub(in crate::process::windows::files) fn check_ordinary_security(
        &self,
        memory_space: &AddressSpace,
        api: Api,
        args: &[u32],
        token: &Token32,
    ) -> Result<(), FileError32> {
        use Api::*;
        let path = |pointer| {
            memory::path(
                memory_space,
                pointer,
                api.name().ends_with('W'),
                self.ordinary.ansi,
            )
        };
        match api {
            DeleteFileA | DeleteFileW | RemoveDirectoryA | RemoveDirectoryW => {
                self.vfs.check_delete_access(&path(args[0])?, token)
            }
            SetFileAttributesA | SetFileAttributesW => {
                self.vfs.check_path_access(&path(args[0])?, 0x100, token)
            }
            GetFileAttributesA | GetFileAttributesW | GetFileAttributesExA
            | GetFileAttributesExW => self.vfs.check_path_access(&path(args[0])?, 0x80, token),
            CopyFileA | CopyFileW | CopyFileExA | CopyFileExW => {
                self.vfs.check_path_access(
                    &path(args[0])?,
                    if matches!(api, CopyFileExA | CopyFileExW) && args[5] & 4 != 0 {
                        3
                    } else {
                        1
                    },
                    token,
                )?;
                let target = path(args[1])?;
                if self.vfs.path_info(&target).is_ok() {
                    self.vfs.check_path_access(&target, 2, token)
                } else {
                    self.vfs.check_parent_access(&target, 2, token)
                }
            }
            MoveFileA
            | MoveFileW
            | MoveFileExA
            | MoveFileExW
            | MoveFileWithProgressA
            | MoveFileWithProgressW => {
                let source = path(args[0])?;
                self.vfs.check_delete_access(&source, token)?;
                let directory = self.vfs.path_info(&source)?.attributes & ATTR_DIRECTORY != 0;
                let target = path(args[1])?;
                self.vfs
                    .check_parent_access(&target, if directory { 4 } else { 2 }, token)?;
                if self.vfs.path_info(&target).is_ok() {
                    self.vfs.check_delete_access(&target, token)?;
                }
                Ok(())
            }
            ReplaceFileA | ReplaceFileW => {
                let replaced = path(args[0])?;
                let replacement = path(args[1])?;
                self.vfs.check_path_access(&replaced, 1, token)?;
                self.vfs.check_delete_access(&replaced, token)?;
                self.vfs
                    .check_path_access(&replacement, 3 | 0x40000, token)?;
                self.vfs.check_delete_access(&replacement, token)?;
                if args[2] != 0 {
                    let backup = path(args[2])?;
                    self.vfs.check_parent_access(&backup, 2, token)?;
                    if self.vfs.path_info(&backup).is_ok() {
                        self.vfs.check_delete_access(&backup, token)?;
                    }
                }
                Ok(())
            }
            FindFirstFileA | FindFirstFileW | FindFirstFileExA | FindFirstFileExW => {
                self.vfs.check_find_access(&path(args[0])?, token)
            }
            _ => Ok(()),
        }
    }
}
