use super::super::{
    DirectInputRuntime32, enumeration::PendingEnumeration32, lifetime::PreparedRelease32,
};
use super::DirectInputThreadReply32;
use crate::{
    input::dinput::DirectInputResult32 as H,
    memory::{AddressSpace, GuestAddress, PageRange, WordWrite32},
    windows::synchronization::EventBinding32,
};

pub(in crate::process) struct PreparedDirectInputExit32 {
    release: PreparedRelease32,
    active: bool,
}
impl PreparedDirectInputExit32 {
    pub(in crate::process) fn bindings(&self) -> &[EventBinding32] {
        &self.release.bindings
    }
}
impl DirectInputRuntime32 {
    pub(in crate::process) fn prepare_thread_exit(
        &self,
        memory: &AddressSpace,
        replies: &[&DirectInputThreadReply32],
        include_active: bool,
        ranges: &[PageRange],
        words: &[WordWrite32],
    ) -> Result<PreparedDirectInputExit32, H> {
        let pending = || {
            replies
                .iter()
                .flat_map(|reply| reply.pending.iter())
                .chain(self.pending.iter().filter(move |_| include_active))
        };
        let capacity = pending().try_fold(0usize, |count, entry| {
            count
                .checked_add(entry.effects.as_ref().map_or(0, Vec::len))
                .and_then(|count| count.checked_add(usize::from(entry.semantics.is_some()) * 2))
                .ok_or(H::OUT_OF_MEMORY)
        })?;
        let mut releases = Vec::new();
        releases
            .try_reserve_exact(capacity)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for entry in pending() {
            append_references(entry, &mut releases);
        }
        let release = self.prepare_releases(memory, &releases, ranges, words)?;
        Ok(PreparedDirectInputExit32 {
            release,
            active: include_active,
        })
    }
    pub(in crate::process) fn commit_thread_exit(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedDirectInputExit32,
    ) -> Result<(), H> {
        self.commit_releases(memory, prepared.release)?;
        if prepared.active {
            self.pending.clear();
        }
        Ok(())
    }
}
fn append_references(pending: &PendingEnumeration32, output: &mut Vec<GuestAddress>) {
    if let Some(effects) = &pending.effects {
        output.extend_from_slice(effects)
    }
    if let Some(semantics) = &pending.semantics {
        output.push(semantics.parent);
        output.extend(semantics.current);
    }
}
