use super::super::{FileError32, FileRuntime32, ordinary::memory};
use crate::{
    files::{FileAccess32, FileId},
    memory::AddressSpace,
    windows::security::{SecurityDescriptor32, Token32},
};

pub(super) enum SecurityInfoTarget32 {
    File(FileId),
    Pipe(crate::windows::files::pipes::PipeEndpointId32),
}
impl FileRuntime32 {
    pub(super) fn security_info_target(
        &self,
        memory_space: &AddressSpace,
        named: bool,
        wide: bool,
        args: &[u32],
    ) -> Result<(SecurityInfoTarget32, Option<u32>), FileError32> {
        if args[1] != 1 {
            return Err(FileError32::Unsupported);
        }
        if named {
            let path = memory::path(memory_space, args[0], wide, self.ordinary.ansi)?;
            if crate::windows::files::pipes::pipe_name(&path)?.is_some() {
                return Ok((
                    SecurityInfoTarget32::Pipe(self.pipes.security_endpoint(&path)?),
                    None,
                ));
            }
            Ok((
                SecurityInfoTarget32::File(self.vfs.path_info(&path)?.id),
                None,
            ))
        } else {
            let access = self.handles.access(args[0])?;
            Ok((
                match self.handles.object(args[0])? {
                    super::super::handles::KernelFileObject32::File(open) => {
                        SecurityInfoTarget32::File(self.vfs.file_for_open(open)?)
                    }
                    super::super::handles::KernelFileObject32::Pipe(pipe) => {
                        SecurityInfoTarget32::Pipe(pipe)
                    }
                    _ => return Err(FileError32::InvalidHandle),
                },
                Some(FileAccess32(access).windows_access() | (access & 0x00100000)),
            ))
        }
    }
    pub(in crate::process) fn query_file_security_info(
        &self,
        memory_space: &AddressSpace,
        named: bool,
        wide: bool,
        args: &[u32],
        token: &Token32,
    ) -> Result<SecurityDescriptor32, FileError32> {
        let (file, granted) = self.security_info_target(memory_space, named, wide, args)?;
        match file {
            SecurityInfoTarget32::File(file) => {
                self.vfs.file_security(file, args[2], token, granted)
            }
            SecurityInfoTarget32::Pipe(pipe) => {
                self.pipes.query_security(pipe, args[2], token, granted)
            }
        }
    }
    pub(in crate::process) fn set_file_security_info(
        &mut self,
        memory_space: &AddressSpace,
        named: bool,
        wide: bool,
        args: &[u32],
        token: &Token32,
    ) -> Result<(), FileError32> {
        let (file, granted) = self.security_info_target(memory_space, named, wide, args)?;
        let descriptor = crate::process::windows::security::memory::descriptor_parts(
            memory_space,
            args[2],
            &args[3..7],
        )?;
        if let SecurityInfoTarget32::Pipe(pipe) = file {
            let prepared = self
                .pipes
                .prepare_security(pipe, args[2], descriptor, token, granted)?;
            self.pipes.commit_security(prepared);
            return Ok(());
        }
        let SecurityInfoTarget32::File(file) = file else {
            unreachable!()
        };
        let propagate = if named {
            true
        } else {
            let open = self.handles.file(args[0])?;
            !self.vfs.open_maximum_allowed(open)? && self.vfs.open_options(open)?.share.0 != 0
        };
        let prepared = self.vfs.prepare_file_security_with_inheritance(
            file, args[2], descriptor, token, granted, propagate,
        )?;
        self.vfs.commit_file_security(prepared);
        Ok(())
    }
}
