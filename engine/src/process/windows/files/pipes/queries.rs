use super::super::{FileCallOutcome32, FileRuntime32, ordinary::memory as guest};
use crate::{
    files::{FileAccess32, FileError32},
    memory::{AddressSpace, GuestAddress},
    windows::{files::FileApi32 as Api, security::Token32},
};

impl FileRuntime32 {
    pub(super) fn query_pipe(
        &mut self,
        memory: &mut AddressSpace,
        api: Api,
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let pipe = self.pipe_handle(args[0], 0)?;
        let options = self.pipes.options(pipe)?;
        if api == Api::SetNamedPipeHandleState {
            if self.handles.access(args[0])?
                & (FileAccess32::WRITE.0 | FileAccess32::WRITE_ATTRIBUTES.0)
                == 0
            {
                return Err(FileError32::AccessDenied);
            }
            if args[2] != 0 || args[3] != 0 {
                return Err(FileError32::Unsupported);
            }
            if args[1] != 0 {
                let mode = guest::dword(memory, args[1])?;
                self.pipes.set_mode(pipe, mode)?;
            }
            return Ok(FileCallOutcome32::success(1));
        }
        let mut spans = Vec::new();
        spans
            .try_reserve_exact(6)
            .map_err(|_| FileError32::Allocation)?;
        let mut add = |address: u32, value: u32| -> Result<(), FileError32> {
            if address != 0 {
                guest::output(memory, address, 4)?;
                spans.push((GuestAddress(address), value.to_le_bytes()));
            }
            Ok(())
        };
        let mut data = None;
        let mut name = None;
        match api {
            Api::PeekNamedPipe => {
                let (bytes, available, left) = self
                    .pipes
                    .peek(pipe, if args[1] == 0 { 0 } else { args[2] as usize })?;
                if args[1] != 0 {
                    guest::output(memory, args[1], bytes.len())?;
                }
                add(args[3], bytes.len() as u32)?;
                add(args[4], available)?;
                add(args[5], left)?;
                data = Some((GuestAddress(args[1]), bytes));
            }
            Api::GetNamedPipeInfo => {
                add(
                    args[1],
                    u32::from(options.server) | if options.message { 4 } else { 0 },
                )?;
                add(args[2], options.out_buffer)?;
                add(args[3], options.in_buffer)?;
                add(args[4], options.max_instances)?;
            }
            Api::GetNamedPipeHandleStateA | Api::GetNamedPipeHandleStateW => {
                add(
                    args[1],
                    u32::from(options.nonblocking) | if options.read_message { 2 } else { 0 },
                )?;
                add(args[2], self.pipes.current_instances(pipe)?)?;
                if args[3] != 0 || args[4] != 0 {
                    return Err(FileError32::Unsupported);
                }
                if args[5] != 0 {
                    if !options.server {
                        return Err(FileError32::InvalidParameter);
                    }
                    let units: Vec<u16> = token.user_name().encode_utf16().collect();
                    let wide = api == Api::GetNamedPipeHandleStateW;
                    let bytes = guest::encoded(&units, wide, true, self.ordinary.ansi)?;
                    let capacity = args[6] as usize * if wide { 2 } else { 1 };
                    if bytes.len() > capacity {
                        return Err(FileError32::InvalidParameter);
                    }
                    guest::output(memory, args[5], bytes.len())?;
                    name = Some((GuestAddress(args[5]), bytes));
                }
            }
            _ => return Err(FileError32::InvalidParameter),
        }
        let mut output: Vec<_> = spans
            .iter()
            .map(|(address, bytes)| (*address, bytes.as_slice()))
            .collect();
        if let Some((address, bytes)) = &data {
            output.push((*address, bytes));
        }
        if let Some((address, bytes)) = &name {
            output.push((*address, bytes));
        }
        if !output.is_empty() {
            memory
                .ensure_transaction_versions(1)
                .map_err(|_| FileError32::Exhausted)?;
            memory
                .write_spans(&output)
                .map_err(|_| FileError32::NoAccess)?;
        }
        Ok(FileCallOutcome32::success(1))
    }
}
