use super::super::{
    FileCallOutcome32, FileRuntime32, handles::KernelFileObject32, ordinary::memory as guest,
};
use crate::{
    files::{FileAccess32, FileError32},
    memory::{Access, AddressSpace, GuestAddress},
    windows::{
        files::{
            FileApi32 as Api,
            completion::{CompletionPacket32, CompletionTarget32},
        },
        synchronization::SynchronizationContext32,
    },
};
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn transfer_null_device(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let KernelFileObject32::Null(id) = self.handles.object(args[0])? else {
            return Err(FileError32::InvalidHandle);
        };
        let write = api == Api::WriteFile;
        let rights = if write {
            FileAccess32::WRITE.0
        } else {
            FileAccess32::READ.0
        };
        if self.handles.access(args[0])? & rights == 0 {
            return Err(FileError32::AccessDenied);
        }
        let asynchronous = self.null_devices.overlapped(id)?;
        if asynchronous && args[4] == 0 || args[3] == 0 && args[4] == 0 {
            return Err(FileError32::InvalidParameter);
        }
        if args[2] != 0 {
            memory
                .check_span(
                    GuestAddress(args[1]),
                    args[2] as usize,
                    if write { Access::Read } else { Access::Write },
                )
                .map_err(|_| FileError32::NoAccess)?;
        }
        if args[3] != 0 {
            guest::output(memory, args[3], 4)?;
        }
        let target = CompletionTarget32::Null(id);
        let modes = self.completion.modes(target);
        let mut events = [0; 2];
        let mut event_count = 0;
        let mut skip = false;
        if args[4] != 0 {
            let raw = guest::read::<20>(memory, args[4])?;
            guest::output(memory, args[4], 8)?;
            let event = u32::from_le_bytes(raw[16..].try_into().unwrap());
            skip = event & 1 != 0;
            if event & !1 != 0 {
                events[event_count] = event & !1;
                event_count += 1;
            }
        }
        if modes & 2 == 0
            && let Some((_, event)) = self
                .events
                .iter()
                .find(|(description, _)| *description == target)
        {
            events[event_count] = *event;
            event_count += 1;
        }
        let signals = if event_count == 0 {
            None
        } else {
            Some(
                sync.as_deref()
                    .ok_or(FileError32::Unsupported)?
                    .prepare_signal_handles(&events[..event_count])
                    .map_err(super::super::asynchronous::synchronization_error)?,
            )
        };
        let bytes = if write { args[2] } else { 0 };
        let error = if !write && args[4] != 0 { 38 } else { 0 };
        let completion = if write && args[4] != 0 && !skip && modes & 1 == 0 {
            self.completion
                .association(target)
                .map(|(port, key, _)| {
                    self.completion.prepare_post(
                        port,
                        CompletionPacket32 {
                            bytes,
                            key,
                            overlapped: args[4],
                            status: 0,
                            error: 0,
                        },
                    )
                })
                .transpose()?
        } else {
            None
        };
        let mut completed = [0; 8];
        completed[..4].copy_from_slice(&(if write { 0u32 } else { 0xc0000011 }).to_le_bytes());
        completed[4..].copy_from_slice(&bytes.to_le_bytes());
        let count = bytes.to_le_bytes();
        if args[3] != 0 || args[4] != 0 {
            memory
                .ensure_transaction_versions(1)
                .map_err(|_| FileError32::Exhausted)?;
            memory
                .write_spans(&[
                    (
                        GuestAddress(args[4]),
                        if args[4] != 0 {
                            completed.as_slice()
                        } else {
                            &[]
                        },
                    ),
                    (
                        GuestAddress(args[3]),
                        if args[3] != 0 { count.as_slice() } else { &[] },
                    ),
                ])
                .map_err(|_| FileError32::NoAccess)?;
        }
        if let Some(signals) = signals {
            sync.unwrap().commit_signals(signals);
        }
        if let Some(completion) = completion {
            self.completion.commit_post(completion);
        }
        Ok(if error == 0 {
            FileCallOutcome32::success(1)
        } else {
            FileCallOutcome32::error(0, error)
        })
    }
}
