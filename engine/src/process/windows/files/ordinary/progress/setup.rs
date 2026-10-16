use super::{CopyStage32, PendingCopy32};
use crate::{
    files::{
        ATTR_DIRECTORY, ATTR_HIDDEN, ATTR_NORMAL, ATTR_READONLY, ATTR_SYSTEM, FileAccess32,
        FileDisposition32, FileError32, FileOpenOptions32, FileShare32,
    },
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        handles::KernelFileObject32,
        ordinary::{FileCallOutcome32, memory},
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn begin_copy_callback(
        &mut self,
        memory: &AddressSpace,
        args: &[u32],
        wide: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[5] & !0x1000_180f != 0 {
            return Err(FileError32::Unsupported);
        }
        memory
            .check_span(GuestAddress(args[2]), 1, Access::Execute)
            .map_err(|_| FileError32::NoAccess)?;
        if args[4] != 0 && memory::dword(memory, args[4])? != 0 {
            return Ok(FileCallOutcome32::error(0, 1235));
        }
        let source = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
        let destination = memory::path(memory, args[1], wide, self.ordinary.ansi)?;
        let info = self.vfs.path_info(&source)?;
        if info.attributes & ATTR_DIRECTORY != 0 {
            return Err(FileError32::AccessDenied);
        }
        let mut destination_attributes = ATTR_NORMAL;
        match self.vfs.path_info(&destination) {
            Ok(target) => {
                if target.id == info.id {
                    return Err(FileError32::SharingViolation);
                }
                if args[5] & 1 != 0 {
                    return Err(FileError32::FileExists);
                }
                if target.attributes & (ATTR_READONLY | ATTR_HIDDEN | ATTR_DIRECTORY) != 0 {
                    return Err(FileError32::AccessDenied);
                }
                destination_attributes = target.attributes & ATTR_SYSTEM;
                if destination_attributes == 0 {
                    destination_attributes = ATTR_NORMAL;
                }
                if self.mappings.minimum_file_size(target.id) > 0 {
                    return Err(FileError32::UserMappedFile);
                }
            }
            Err(FileError32::NotFound) => (),
            Err(error) => return Err(error),
        }
        self.ordinary
            .copies
            .try_reserve(1)
            .map_err(|_| crate::files::FileError32::Allocation)?;
        if self.ordinary.copies.len() == 33 {
            return Err(FileError32::Capacity);
        }
        let continuation = self
            .ordinary
            .sequence
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        self.vfs
            .revision()
            .checked_add(5)
            .ok_or(FileError32::Exhausted)?;
        self.handles.ensure_capacity(2)?;
        self.vfs.ensure_open_capacity(2)?;
        let source_options = FileOpenOptions32 {
            access: FileAccess32(
                FileAccess32::READ.0
                    | if args[5] & 4 != 0 {
                        FileAccess32::WRITE.0
                    } else {
                        0
                    },
            ),
            share: FileShare32::ALL,
            ..FileOpenOptions32::default()
        };
        let destination_options = FileOpenOptions32 {
            access: FileAccess32(
                FileAccess32::WRITE.0 | FileAccess32::DELETE.0 | FileAccess32::READ.0,
            ),
            share: FileShare32::ALL,
            disposition: if args[5] & 1 != 0 {
                FileDisposition32::CreateNew
            } else {
                FileDisposition32::CreateAlways
            },
            attributes: destination_attributes,
            ..FileOpenOptions32::default()
        };
        let source_receipt = self.vfs.prepare_open(&source, source_options)?;
        self.vfs.prepare_open(&destination, destination_options)?;
        source_receipt.validate(&self.vfs)?;
        let source_open = self.vfs.commit_open(source_receipt).open;
        let destination_receipt = match self.vfs.prepare_open(&destination, destination_options) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.vfs
                    .close_open(source_open)
                    .expect("reserved source cleanup");
                return Err(error);
            }
        };
        let destination_result = self.vfs.commit_open(destination_receipt);
        let destination_open = destination_result.open;
        self.vfs
            .retain_open(source_open)
            .expect("fresh source reference");
        self.vfs
            .retain_open(destination_open)
            .expect("fresh destination reference");
        let source_handle = self
            .handles
            .prepare(source_options.access.0 | 0x0010_0000, false)
            .expect("reserved source handle");
        let source_handle = self
            .handles
            .commit(source_handle, KernelFileObject32::File(source_open));
        let destination_handle = self
            .handles
            .prepare(destination_options.access.0 | 0x0010_0000, false)
            .expect("reserved destination handle");
        let destination_handle = self.handles.commit(
            destination_handle,
            KernelFileObject32::File(destination_open),
        );
        let copy = PendingCopy32 {
            continuation,
            procedure: GuestAddress(args[2]),
            data: args[3],
            cancel: args[4],
            source: source_open,
            destination: destination_open,
            source_file: info.id,
            destination_file: destination_result.file,
            source_handle,
            destination_handle,
            size: info.size,
            stage: CopyStage32::Start,
        };
        self.ordinary.sequence = continuation;
        self.ordinary.copies.push(copy);
        Ok(FileCallOutcome32::Callback(copy.callback()))
    }
}
