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
    memory
        .cpu_paging_check32(address, length as usize, intent, backing)
        .map_err(|error| paging_error(memory, error))
}

pub(crate) fn read_cpu_paging_access32(
    memory: &AddressSpace,
    address: GuestAddress,
    output: &mut [u8],
    access: PagingAccess32,
) -> Result<(), CpuPagingError32> {
    u32::try_from(output.len()).map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?;
    let intent = match access {
        PagingAccess32::Read => Access::Read,
        PagingAccess32::Write => Access::Write,
        PagingAccess32::Execute => Access::Execute,
    };
    memory
        .cpu_paging_read32(address, output, intent)
        .map_err(|error| paging_error(memory, error))
}

fn paging_error(memory: &AddressSpace, error: PagingCheckError32) -> CpuPagingError32 {
    match error {
        PagingCheckError32::Memory(error) => CpuPagingError32::Memory(error),
        PagingCheckError32::Fault(facts) => {
            if facts.identity() != memory.identity() {
                return CpuPagingError32::Memory(MemoryError::InvalidRange);
            }
            let access = match facts.access() {
                Access::Read => PagingAccess32::Read,
                Access::Write => PagingAccess32::Write,
                Access::Execute => PagingAccess32::Execute,
            };
            CpuPagingError32::PageFault(VerifiedPageFault32::application(
                facts.linear(),
                access,
                facts.present(),
            ))
        }
    }
}
