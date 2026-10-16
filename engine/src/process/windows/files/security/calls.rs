use super::super::{FileError32, FileRuntime32, ordinary::memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::security::{SecurityApi32 as Api, Token32},
};

impl FileRuntime32 {
    pub(in crate::process) fn execute_file_security(
        &mut self,
        memory_space: &mut AddressSpace,
        api: Api,
        args: &[u32],
        token: &Token32,
    ) -> Result<(u32, Option<u32>), FileError32> {
        let path = memory::path(
            memory_space,
            args[0],
            api.name().ends_with('W'),
            self.ordinary.ansi,
        )?;
        let file = self.vfs.path_info(&path)?.id;
        match api {
            Api::GetFileSecurityA | Api::GetFileSecurityW => {
                let descriptor = self.vfs.file_security(file, args[1], token, None)?;
                let bytes = descriptor.to_self_relative()?;
                memory::output(memory_space, args[4], 4)?;
                if (args[3] as usize) < bytes.len() {
                    memory_space
                        .write(GuestAddress(args[4]), &(bytes.len() as u32).to_le_bytes())
                        .map_err(|_| FileError32::NoAccess)?;
                    return Ok((0, Some(122)));
                }
                memory::output(memory_space, args[2], bytes.len())?;
                memory_space
                    .write(GuestAddress(args[2]), &bytes)
                    .map_err(|_| FileError32::NoAccess)?;
                memory_space
                    .write(GuestAddress(args[4]), &(bytes.len() as u32).to_le_bytes())
                    .map_err(|_| FileError32::NoAccess)?;
                Ok((1, None))
            }
            Api::SetFileSecurityA | Api::SetFileSecurityW => {
                let supplied =
                    crate::process::windows::security::memory::descriptor(memory_space, args[2])?;
                let prepared = self
                    .vfs
                    .prepare_file_security(file, args[1], supplied, token, None)?;
                self.vfs.commit_file_security(prepared);
                Ok((1, None))
            }
            _ => Err(FileError32::InvalidParameter),
        }
    }
}
