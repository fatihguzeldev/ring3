use super::PendingCopy32;
use crate::{
    files::{FileError32, OpenFileId},
    process::windows::files::{FileRuntime32, ordinary::OrdinaryThreadReply32},
};

pub(in crate::process::windows::files) struct PreparedCopyThreadExit32 {
    copies: Vec<PendingCopy32>,
    closes: Vec<(OpenFileId, u32)>,
    final_numeric: Vec<OpenFileId>,
    events: Vec<(crate::windows::files::completion::CompletionTarget32, u32)>,
    event_handles: Vec<u32>,
    active: bool,
}
impl PreparedCopyThreadExit32 {
    pub(in crate::process::windows::files) fn closes(&self) -> &[(OpenFileId, u32)] {
        &self.closes
    }
    pub(in crate::process::windows::files) fn event_handles(&self) -> &[u32] {
        &self.event_handles
    }
}
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn prepare_copy_thread_exit(
        &self,
        saved: &[&OrdinaryThreadReply32],
        active: bool,
    ) -> Result<PreparedCopyThreadExit32, FileError32> {
        let count = saved.iter().try_fold(
            if active {
                self.ordinary.copies.len()
            } else {
                0
            },
            |count, reply| {
                count
                    .checked_add(reply.copies().len())
                    .ok_or(FileError32::Capacity)
            },
        )?;
        let mut copies: Vec<PendingCopy32> = Vec::new();
        copies
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for copy in saved
            .iter()
            .flat_map(|reply| reply.copies())
            .chain(self.ordinary.copies.iter().filter(|_| active))
        {
            if copies
                .iter()
                .any(|existing| existing.continuation == copy.continuation)
            {
                return Err(FileError32::InvalidParameter);
            }
            copies.push(*copy);
        }
        let capacity = count.checked_mul(2).ok_or(FileError32::Capacity)?;
        let mut closes: Vec<(OpenFileId, u32)> = Vec::new();
        let mut numeric: Vec<(OpenFileId, usize)> = Vec::new();
        let mut final_numeric = Vec::new();
        let mut events = Vec::new();
        let mut event_handles = Vec::new();
        closes
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        numeric
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        final_numeric
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        events
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        event_handles
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        for copy in &copies {
            for (handle, open) in [
                (copy.source_handle, copy.source),
                (copy.destination_handle, copy.destination),
            ] {
                self.vfs.open_info(open)?;
                let visible = usize::from(self.handles.file(handle) == Ok(open));
                if let Some((_, count)) = numeric.iter_mut().find(|(id, _)| *id == open) {
                    *count = count.checked_add(visible).ok_or(FileError32::Exhausted)?;
                } else {
                    numeric.push((open, visible));
                }
                let count = 1 + visible as u32;
                if let Some((_, total)) = closes.iter_mut().find(|(id, _)| *id == open) {
                    *total = total.checked_add(count).ok_or(FileError32::Exhausted)?;
                } else {
                    closes.push((open, count));
                }
            }
        }
        for (open, removed) in numeric {
            if self.handles.file_count(open) != removed {
                continue;
            }
            final_numeric.push(open);
            if let Some(&(id, handle)) = self.events.iter().find(|(id, _)| {
                *id == crate::windows::files::completion::CompletionTarget32::File(open)
            }) {
                events.push((id, handle));
                event_handles.push(handle);
            }
        }
        Ok(PreparedCopyThreadExit32 {
            copies,
            closes,
            final_numeric,
            events,
            event_handles,
            active,
        })
    }

    pub(in crate::process::windows::files) fn commit_copy_thread_exit(
        &mut self,
        prepared: PreparedCopyThreadExit32,
    ) {
        for copy in &prepared.copies {
            for (handle, open) in [
                (copy.source_handle, copy.source),
                (copy.destination_handle, copy.destination),
            ] {
                if self.handles.file(handle) == Ok(open) {
                    self.handles
                        .remove(handle)
                        .expect("prepared thread copy handle");
                    self.vfs
                        .close_open(open)
                        .expect("prepared thread copy visible reference");
                }
                self.vfs
                    .close_open(open)
                    .expect("prepared thread copy hidden reference");
            }
        }
        for open in prepared.final_numeric {
            assert_eq!(self.handles.file_count(open), 0);
            self.asynchronous.cancel_open(open);
            self.release_file_locks(open);
        }
        self.events.retain(|event| !prepared.events.contains(event));
        if prepared.active {
            self.ordinary.copies.retain(|copy| {
                !prepared
                    .copies
                    .iter()
                    .any(|retired| retired.continuation == copy.continuation)
            });
        }
    }
}
