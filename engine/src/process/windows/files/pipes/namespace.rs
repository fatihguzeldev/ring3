use super::super::{
    FileCallOutcome32, FileRuntime32,
    handles::KernelFileObject32,
    ordinary::{access, memory as guest},
};
use crate::{
    files::{FileAccess32, FileError32},
    memory::{AddressSpace, GuestAddress},
    windows::{
        files::pipes::{PipeOptions32, pipe_name},
        security::Token32,
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn open_pipe_client(
        &mut self,
        memory: &AddressSpace,
        path: &[u16],
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[4] != 3 || args[6] != 0 || args[5] & !(0xc000_0000 | 0x0000_0080) != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let inherit = guest::inherit(memory, args[3])?;
        let prepared =
            self.pipes
                .prepare_client(path, args[1], args[5] & 0x4000_0000 != 0, token)?;
        let rights = access::desired_handle_access(prepared.granted_access())?;
        let handle = self.handles.prepare(rights, inherit)?;
        let endpoint = self.pipes.commit_client(prepared);
        let handle = self
            .handles
            .commit(handle, KernelFileObject32::Pipe(endpoint));
        Ok(FileCallOutcome32::success(handle))
    }
    pub(super) fn create_named_pipe(
        &mut self,
        memory: &AddressSpace,
        wide: bool,
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let path = guest::path(memory, args[0], wide, self.ordinary.ansi)?;
        pipe_name(&path)?.ok_or(FileError32::InvalidName)?;
        if args[1] & !(3 | 0xc000_0000 | 0x000c_0000) != 0 || args[2] & !7 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let inherit = guest::inherit(memory, args[7])?;
        let descriptor =
            crate::process::windows::security::memory::attributes_descriptor(memory, args[7])?
                .map_or_else(|| crate::windows::files::pipes::default_security(token), Ok)?;
        let options = PipeOptions32 {
            access: args[1] & 3,
            message: args[2] & 4 != 0,
            read_message: args[2] & 2 != 0,
            nonblocking: args[2] & 1 != 0,
            overlapped: args[1] & 0x4000_0000 != 0,
            first: args[1] & 0x0008_0000 != 0,
            maximum: args[3],
            out_buffer: args[4] as usize,
            in_buffer: args[5] as usize,
            timeout: args[6],
        };
        let prepared = self
            .pipes
            .prepare_server(&path, options, descriptor, token)?;
        let raw = (if options.access & 1 != 0 {
            0x8000_0000
        } else {
            0
        }) | (if options.access & 2 != 0 {
            0x4000_0000
        } else {
            0
        }) | (args[1] & 0x000c_0000);
        let handle = self
            .handles
            .prepare(access::desired_handle_access(raw)?, inherit)?;
        let (pipe, _) = self.pipes.commit_create(prepared);
        let handle = self.handles.commit(handle, KernelFileObject32::Pipe(pipe));
        Ok(FileCallOutcome32::success(handle))
    }
    pub(super) fn create_anonymous_pipe(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        guest::output(memory, args[0], 4)?;
        guest::output(memory, args[1], 4)?;
        if args[0] == args[1] {
            return Err(FileError32::InvalidParameter);
        }
        let inherit = guest::inherit(memory, args[2])?;
        let descriptor =
            crate::process::windows::security::memory::attributes_descriptor(memory, args[2])?
                .map_or_else(|| crate::windows::files::pipes::default_security(token), Ok)?;
        let prepared = self.pipes.prepare_anonymous(
            if args[3] == 0 { 4096 } else { args[3] as usize },
            descriptor,
        )?;
        let [read, write] = self.handles.prepare_pair(
            [
                access::desired_handle_access(0x8000_0000)?,
                access::desired_handle_access(0x4000_0000)?,
            ],
            inherit,
        )?;
        let values = [read.handle().to_le_bytes(), write.handle().to_le_bytes()];
        memory
            .ensure_transaction_versions(1)
            .map_err(|_| FileError32::Exhausted)?;
        memory
            .write_spans(&[
                (GuestAddress(args[0]), &values[0]),
                (GuestAddress(args[1]), &values[1]),
            ])
            .map_err(|_| FileError32::NoAccess)?;
        let (reader, writer) = self.pipes.commit_create(prepared);
        self.handles.commit(read, KernelFileObject32::Pipe(reader));
        self.handles
            .commit(write, KernelFileObject32::Pipe(writer.unwrap()));
        Ok(FileCallOutcome32::success(1))
    }
    pub(super) fn pipe_handle(
        &self,
        handle: u32,
        rights: u32,
    ) -> Result<crate::windows::files::pipes::PipeEndpointId32, FileError32> {
        let KernelFileObject32::Pipe(pipe) = self.handles.object(handle)? else {
            return Err(FileError32::InvalidHandle);
        };
        if self.handles.access(handle)? & rights != rights {
            return Err(FileError32::AccessDenied);
        }
        self.pipes.options(pipe)?;
        Ok(pipe)
    }
    pub(in crate::process::windows::files) fn pipe_metadata(
        &mut self,
        memory: &mut AddressSpace,
        api: crate::windows::files::FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let pipe = self.pipe_handle(args[0], 0)?;
        if api == crate::windows::files::FileApi32::GetFileType {
            return Ok(FileCallOutcome32::success(3));
        }
        if api == crate::windows::files::FileApi32::FlushFileBuffers {
            if self.handles.access(args[0])? & FileAccess32::WRITE.0 == 0 {
                return Err(FileError32::AccessDenied);
            }
            if self.pipes.flush_ready(pipe) {
                self.pipe_wait = None;
                return Ok(FileCallOutcome32::success(1));
            }
            self.pipe_wait = Some(super::PipeWait32::Flush(pipe));
            return Ok(FileCallOutcome32::Pending);
        }
        let _ = memory;
        Err(FileError32::Unsupported)
    }
}
