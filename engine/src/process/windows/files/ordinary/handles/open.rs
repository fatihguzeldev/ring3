use super::{FileCallOutcome32, access::desired_access, memory};
use crate::{
    files::{ATTR_NORMAL, FileDisposition32, FileError32, FileOpenOptions32, FileShare32},
    memory::AddressSpace,
    process::windows::files::{FileRuntime32, handles::KernelFileObject32},
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn open_file(
        &mut self,
        memory: &AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        self.open_file_with_security(
            memory,
            api,
            args,
            &crate::windows::security::Token32::virtual_process(),
        )
    }
    pub(in crate::process::windows::files) fn open_file_with_security(
        &mut self,
        memory: &AddressSpace,
        api: FileApi32,
        args: &[u32],
        token: &crate::windows::security::Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let path = memory::path(
            memory,
            args[0],
            api == FileApi32::CreateFileW,
            self.ordinary.ansi,
        )?;
        let access = desired_access(args[1])?;
        if args[2] & !7 != 0 || args[5] & !0xffb0_ffff != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let disposition = match args[4] {
            1 => FileDisposition32::CreateNew,
            2 => FileDisposition32::CreateAlways,
            3 => FileDisposition32::OpenExisting,
            4 => FileDisposition32::OpenAlways,
            5 => FileDisposition32::TruncateExisting,
            _ => return Err(FileError32::InvalidParameter),
        };
        let mut attributes = args[5] & 0xffff;
        if attributes & !0x31b7 != 0 {
            return Err(FileError32::Unsupported);
        }
        if attributes == 0 {
            attributes = ATTR_NORMAL;
        }
        if attributes & !ATTR_NORMAL != 0 {
            attributes &= !ATTR_NORMAL;
        }
        if args[6] != 0 {
            attributes |= self.vfs.open_info(self.handles.file(args[6])?)?.attributes & !0x10;
        }
        let inherit = memory::inherit(memory, args[3])?;
        if matches!(
            disposition,
            FileDisposition32::CreateAlways | FileDisposition32::TruncateExisting
        ) && let Ok(info) = self.vfs.path_info(&path)
            && self.mappings.minimum_file_size(info.id) > 0
        {
            return Err(FileError32::UserMappedFile);
        }
        let descriptor = if self.vfs.path_info(&path).is_err() {
            crate::process::windows::security::memory::attributes_descriptor(memory, args[3])?
        } else {
            None
        };
        let prepared = self.vfs.prepare_open_with_security(
            &path,
            FileOpenOptions32 {
                access,
                share: FileShare32(args[2]),
                disposition,
                attributes,
                directory: args[5] & 0x0200_0000 != 0,
                delete_on_close: args[5] & 0x0400_0000 != 0,
                overlapped: args[5] & 0x4000_0000 != 0,
                no_buffering: args[5] & 0x2000_0000 != 0,
            },
            args[1],
            token,
            descriptor,
        )?;
        let grant = prepared.granted_access();
        let handle = self.handles.prepare(
            crate::files::FileAccess32::from_windows(grant).0 | (grant & 0x0010_0000),
            inherit,
        )?;
        prepared.validate(&self.vfs)?;
        let opened = self.vfs.commit_open(prepared);
        let handle = self
            .handles
            .commit(handle, KernelFileObject32::File(opened.open));
        Ok(FileCallOutcome32::Return {
            value: handle,
            last_error: matches!(
                disposition,
                FileDisposition32::CreateAlways | FileDisposition32::OpenAlways
            )
            .then_some(if opened.existed { 183 } else { 0 }),
        })
    }
}
