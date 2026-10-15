use super::{FileCallOutcome32, memory};
use crate::{
    files::{ATTR_DIRECTORY, FileError32, FileIo32},
    memory::AddressSpace,
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn mutate_path(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        let wide = api.name().ends_with('W');
        let source = memory::path(memory, args[0], wide)?;
        let prepared = match api {
            CreateDirectoryA | CreateDirectoryW => {
                memory::inherit(memory, args[1])?;
                self.vfs.prepare_create_directory(&source, 0)?
            }
            CreateDirectoryExA | CreateDirectoryExW => {
                memory::inherit(memory, args[2])?;
                let template = self.vfs.path_info(&source)?;
                if template.attributes & ATTR_DIRECTORY == 0 {
                    return Err(FileError32::NotDirectory);
                }
                let target = memory::path(memory, args[1], wide)?;
                self.vfs
                    .prepare_create_directory(&target, template.attributes)?
            }
            ReplaceFileA | ReplaceFileW => {
                if args[3] & !7 != 0 || args[4] != 0 || args[5] != 0 {
                    return Err(FileError32::InvalidParameter);
                }
                if args[3] & 1 != 0 {
                    return Err(FileError32::Unsupported);
                }
                let replacement = memory::path(memory, args[1], wide)?;
                let backup = if args[2] != 0 {
                    Some(memory::path(memory, args[2], wide)?)
                } else {
                    None
                };
                self.vfs
                    .prepare_replace(&source, &replacement, backup.as_deref())?
            }
            RemoveDirectoryA | RemoveDirectoryW => self.vfs.prepare_remove_directory(&source)?,
            DeleteFileA | DeleteFileW => self.vfs.prepare_delete(&source)?,
            SetFileAttributesA | SetFileAttributesW => {
                self.vfs.prepare_set_attributes(&source, args[1])?
            }
            SetCurrentDirectoryA | SetCurrentDirectoryW => {
                self.vfs.prepare_current_directory(&source)?
            }
            CopyFileA | CopyFileW | CopyFileExA | CopyFileExW => {
                let extended = matches!(api, CopyFileExA | CopyFileExW);
                if extended {
                    if args[2] != 0 {
                        return self.begin_copy_callback(memory, args, wide);
                    }
                    if args[5] & !0x1000_180f != 0 {
                        return Err(FileError32::Unsupported);
                    }
                    if args[4] != 0 && memory::dword(memory, args[4])? != 0 {
                        return Ok(FileCallOutcome32::error(0, 1235));
                    }
                }
                if extended && args[5] & 4 != 0 {
                    self.vfs.prepare_open(
                        &source,
                        crate::files::FileOpenOptions32 {
                            access: crate::files::FileAccess32(
                                crate::files::FileAccess32::READ.0
                                    | crate::files::FileAccess32::WRITE.0,
                            ),
                            ..crate::files::FileOpenOptions32::default()
                        },
                    )?;
                }
                let target = memory::path(memory, args[1], wide)?;
                let source_info = self.vfs.path_info(&source)?;
                self.check_copy_file_lock(source_info.id, source_info.size, false)?;
                if let Ok(info) = self.vfs.path_info(&target) {
                    self.check_copy_file_lock(info.id, source_info.size, true)?;
                    if self.mappings.minimum_file_size(info.id) > 0 {
                        return Err(FileError32::UserMappedFile);
                    }
                }
                match self.vfs.prepare_copy(
                    &source,
                    &target,
                    if extended {
                        args[5] & 1 != 0
                    } else {
                        args[2] != 0
                    },
                )? {
                    FileIo32::Ready(prepared) => prepared,
                    FileIo32::NeedBytes(request) => {
                        return Ok(FileCallOutcome32::PendingBytes(request));
                    }
                }
            }
            MoveFileA
            | MoveFileW
            | MoveFileExA
            | MoveFileExW
            | MoveFileWithProgressA
            | MoveFileWithProgressW => {
                let progress = matches!(api, MoveFileWithProgressA | MoveFileWithProgressW);
                let flags = if progress {
                    args[4]
                } else if matches!(api, MoveFileExA | MoveFileExW) {
                    args[2]
                } else {
                    0
                };
                if flags & !0x2f != 0 || flags & 4 != 0 {
                    return Err(FileError32::Unsupported);
                }
                if args[1] == 0 {
                    return Err(FileError32::InvalidParameter);
                }
                let target = memory::path(memory, args[1], wide)?;
                self.vfs.prepare_move(&source, &target, flags & 1 != 0)?
            }
            _ => return Err(FileError32::Unsupported),
        };
        prepared.validate(&self.vfs)?;
        self.vfs.commit_namespace(prepared);
        Ok(FileCallOutcome32::success(1))
    }
}
