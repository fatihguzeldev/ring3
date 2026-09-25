use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions};

pub(super) const BASE: u32 = 0x7000_3000;

// self-authored cdecl loop: esi walks [start, end), edi holds end; both are saved.
// null entries are skipped; calls and traversal consume the ordinary cpu budget.
const INITTERM: &[u8] = &[
    0x56, 0x57, 0x8b, 0x74, 0x24, 0x0c, 0x8b, 0x7c, 0x24, 0x10, 0x39, 0xfe, 0x73, 0x0e, 0x8b, 0x06,
    0x83, 0xf8, 0, 0x74, 2, 0xff, 0xd0, 0x83, 0xc6, 4, 0xeb, 0xee, 0x5f, 0x5e, 0xc3,
];

pub(super) fn initialize(memory: &mut GuestMemory) -> Result<(), MemoryError> {
    memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
    memory.write(u64::from(BASE), INITTERM)?;
    memory.write(u64::from(super::sorting::ENTRY), super::sorting_code::CODE)?;
    memory.protect(u64::from(BASE), PAGE_SIZE, Permissions::READ_EXECUTE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializer_code_refuses_an_existing_mapping() {
        let mut memory = GuestMemory::new(2);
        memory
            .map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(
            initialize(&mut memory),
            Err(MemoryError::AlreadyMapped {
                address: u64::from(BASE)
            })
        );
    }
}
