use super::*;
use crate::memory::GuestAddress;
impl EngineInstance {
    pub(super) fn execute_handle_information(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        let current = match args[0] {
            u32::MAX | 0xffff_fffe => Ok(0),
            handle => self.file_handle_flags(handle).unwrap_or_else(|| {
                self.synchronization
                    .as_ref()
                    .ok_or(HostError::Synchronization(
                        crate::windows::synchronization::SynchronizationError32::InvalidHandle,
                    ))
                    .and_then(|runtime| {
                        runtime
                            .context()
                            .handle_flags(handle)
                            .map_err(HostError::Synchronization)
                    })
            }),
        };
        let result = match current {
            Err(error) => Err(error),
            Ok(flags) if api == ProcessApi32::GetHandleInformation => self
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(args[1]), &flags.to_le_bytes())
                .map_err(HostError::Memory),
            Ok(_) if matches!(args[0], u32::MAX | 0xffff_fffe) => Ok(()),
            Ok(_) => self
                .set_file_handle_flags(args[0], args[1], args[2])
                .unwrap_or_else(|| {
                    self.synchronization
                        .as_mut()
                        .ok_or(HostError::Synchronization(
                            crate::windows::synchronization::SynchronizationError32::InvalidHandle,
                        ))
                        .and_then(|runtime| {
                            runtime
                                .context_mut()
                                .set_handle_flags(args[0], args[1], args[2])
                                .map_err(HostError::Synchronization)
                        })
                }),
        };
        Ok(match result {
            Ok(()) => 1u32.into(),
            Err(HostError::File(error)) => ProcessReply32 {
                value: 0,
                error: Some(error.win32()),
            },
            Err(HostError::Synchronization(error)) => ProcessReply32 {
                value: 0,
                error: Some(error.win32()),
            },
            Err(error) => return Err(error),
        })
    }
}
