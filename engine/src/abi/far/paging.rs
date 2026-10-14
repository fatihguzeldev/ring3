use super::*;
use crate::{
    cpu::x86::{CpuPagingError32, PagingAccess32, VerifiedPageFault32},
    memory::{GuestAddress, MemoryError, MemoryFault},
};

fn literal(version: u16, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3MH");
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + 4 * index..20 + 4 * index].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}
fn pf(access: PagingAccess32) -> VerifiedPageFault32 {
    VerifiedPageFault32::application(GuestAddress(0), access, false)
}

#[test]
fn gate_stack_paging_keeps_v15_and_wrapped_raw_memory_keeps_v9() {
    for slot in [2, 4] {
        let mut output = [0xa5; 40];
        encode_gate_result(
            slot,
            123,
            Err(CpuMemoryError::Paging(CpuPagingError32::PageFault(pf(
                PagingAccess32::Write,
            )))),
            &mut output,
        )
        .unwrap();
        assert_eq!(output, literal(15, [9, 0, 6, 0, 2, slot]));
        let raw = CpuPagingError32::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0),
            access: Access::Write,
            reason: crate::memory::FaultReason::Unmapped,
        }));
        encode_gate_result(slot, 0, Err(CpuMemoryError::Paging(raw)), &mut output).unwrap();
        assert_eq!(output, literal(9, [1, 0, 1, 0, 2, slot]));
    }
}

#[test]
fn gate_paging_requires_selected_user_write_slot_without_output_mutation() {
    for (slot, error) in [
        (0, CpuPagingError32::PageFault(pf(PagingAccess32::Write))),
        (2, CpuPagingError32::PageFault(pf(PagingAccess32::Read))),
        (1, CpuPagingError32::PageFault(pf(PagingAccess32::Write))),
        (4, CpuPagingError32::PageFault(pf(PagingAccess32::Execute))),
    ] {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_gate_result(slot, 0, Err(CpuMemoryError::Paging(error)), &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
}
