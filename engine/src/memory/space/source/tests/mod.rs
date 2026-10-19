use super::*;

mod failures;
mod publication;

fn page(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

fn stamps(memory: &AddressSpace) -> (u64, u64, u64) {
    (
        memory.transaction_version(),
        memory.code_validation_version(),
        memory.code_source_version(),
    )
}

fn paired(memory: &AddressSpace) {
    assert_eq!(
        stamps(memory),
        (
            memory.transaction_version(),
            memory.transaction_version(),
            memory.transaction_version(),
        )
    );
}

fn read<const N: usize>(memory: &AddressSpace, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}
