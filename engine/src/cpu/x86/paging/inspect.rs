use super::CpuPagingError32;
use crate::{
    cpu::x86::{PagingAccess32, VerifiedPageFault32},
    memory::{
        Access, AddressSpace, GuestAddress, MemoryError, PagingBacking32, PagingCheckError32,
    },
};

pub(crate) fn inspect_cpu_paging_access32(
    memory: &AddressSpace,
    address: GuestAddress,
    length: u32,
    access: PagingAccess32,
    backing: PagingBacking32,
) -> Result<(), CpuPagingError32> {
    let intent = match access {
        PagingAccess32::Read => Access::Read,
        PagingAccess32::Write => Access::Write,
        PagingAccess32::Execute => Access::Execute,
    };
    match memory.cpu_paging_check32(address, length as usize, intent, backing) {
        Ok(()) => Ok(()),
        Err(PagingCheckError32::Memory(error)) => Err(CpuPagingError32::Memory(error)),
        Err(PagingCheckError32::Fault(facts)) => {
            if facts.identity() != memory.identity() {
                return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
            }
            let access = match facts.access() {
                Access::Read => PagingAccess32::Read,
                Access::Write => PagingAccess32::Write,
                Access::Execute => PagingAccess32::Execute,
            };
            Err(CpuPagingError32::PageFault(
                VerifiedPageFault32::application(facts.linear(), access, facts.present()),
            ))
        }
    }
}
