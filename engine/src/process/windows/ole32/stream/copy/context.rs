use crate::{
    memory::{Access, AddressSpace, GuestAddress, MemoryError, PAGE_SIZE, Permissions},
    process::{
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::marshaling::{ExecutionApartment32, Remote32},
        },
    },
    windows::{CallFrame32, ole32::stream::StreamId32},
};
#[derive(Clone, Copy)]
pub(super) struct MappingWitness32 {
    identity: u64,
    pages: [Option<(GuestAddress, u64, Permissions)>; 9],
}
impl MappingWitness32 {
    pub(super) fn capture(
        memory: &AddressSpace,
        spans: &[(u32, usize, Access)],
    ) -> Result<Self, MemoryError> {
        let mut result = Self {
            identity: memory.identity(),
            pages: [None; 9],
        };
        let mut count = 0;
        for &(address, length, access) in spans {
            if address == 0 || length == 0 {
                continue;
            }
            memory.check_span(GuestAddress(address), length, access)?;
            let first = u64::from(address) / u64::from(PAGE_SIZE);
            let last = (u64::from(address) + length as u64 - 1) / u64::from(PAGE_SIZE);
            for index in first..=last {
                let address = GuestAddress(index as u32 * PAGE_SIZE);
                if result.pages[..count]
                    .iter()
                    .flatten()
                    .any(|page| page.0 == address)
                {
                    continue;
                }
                if count == result.pages.len() {
                    return Err(MemoryError::InvalidRange);
                }
                result.pages[count] = Some((
                    address,
                    memory.mapping_version_at(address)?,
                    memory.permissions_at(address)?,
                ));
                count += 1;
            }
        }
        Ok(result)
    }
    pub(super) fn check(&self, memory: &AddressSpace) -> Result<(), MemoryError> {
        if memory.identity() != self.identity {
            return Err(MemoryError::InvalidRange);
        }
        for &(address, mapping, permissions) in self.pages.iter().flatten() {
            if memory.mapping_version_at(address)? != mapping
                || memory.permissions_at(address)? != permissions
            {
                return Err(MemoryError::InvalidRange);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CopyPhase32 {
    Read,
    Write,
    Finish(u32),
}
pub(in crate::process::windows::ole32) struct StreamCopy32 {
    pub(super) continuation: u64,
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(super) execution: ExecutionApartment32,
    pub(super) source: GuestAddress,
    pub(super) id: StreamId32,
    pub(super) destination: u32,
    pub(super) table: u32,
    pub(super) procedure: GuestAddress,
    pub(super) witness: MappingWitness32,
    pub(super) scratch: GuestCallbackScratch32,
    pub(super) remaining: u64,
    pub(super) read: u64,
    pub(super) written: u64,
    pub(super) chunk: u32,
    pub(super) remote: Remote32,
    pub(super) phase: CopyPhase32,
}
pub(super) const CHUNK: usize = 4092;
