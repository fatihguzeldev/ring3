use super::mutate;

mod transport;
pub(super) use transport::{cpu_mutate, cpu_paging_store};

pub(crate) fn cpu_read_modify8(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read_modify8(address))
}

pub(crate) fn cpu_read_modify16(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read_modify16(address))
}

pub(crate) fn cpu_read_modify32(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read_modify32(address))
}

pub(crate) fn cpu_read_modify(address: u32, length: u32, alignment: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read_modify(address, length, alignment))
}

pub(crate) fn cpu_fetch_fault(pc: u32, length: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_fetch_fault(pc, length))
}

pub(crate) fn cpu_fetch_fault_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    pc: u32,
    length: u32,
) -> u32 {
    cpu_mutate(|instance| {
        instance.cpu_fetch_fault_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            pc,
            length,
        )
    })
}

pub(crate) fn read_cr2() -> u32 {
    mutate(|instance| instance.read_cr2_request())
}
