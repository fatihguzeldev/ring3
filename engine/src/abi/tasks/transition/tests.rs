use super::*;

#[test]
fn committed_transition_has_distinct_fault_and_staleness_fields() {
    for (fault, reason, code) in [
        (None, 0_u32, 0_u32),
        (Some(TaskTransitionFault32::InvalidTss(0x4c)), 21, 0x4c),
        (Some(TaskTransitionFault32::NotPresent(0x50)), 14, 0x50),
        (Some(TaskTransitionFault32::StackSegment(0x58)), 15, 0x58),
        (Some(TaskTransitionFault32::GeneralProtection(0)), 11, 0),
    ] {
        for flags in 0..4_u32 {
            let mut bytes = [0xa5; HELPER_SIZE];
            encode_task_transition_result(
                TaskTransitionReply32::Committed {
                    fault,
                    code_invalidated: flags & 1 != 0,
                    task_trap: flags & 2 != 0,
                },
                &mut bytes,
            )
            .unwrap();
            let mut expected = [0; HELPER_SIZE];
            expected[..16].copy_from_slice(b"R3MH\x0e\0\x01\0\x28\0\0\0\0\0\0\0");
            expected[20..24].copy_from_slice(&reason.to_le_bytes());
            expected[24..28].copy_from_slice(&code.to_le_bytes());
            expected[32..36].copy_from_slice(&flags.to_le_bytes());
            assert_eq!(bytes, expected);
        }
    }
}

#[test]
fn malformed_fault_geometry_and_unowned_authority_do_not_write_a_packet() {
    use crate::memory::GuestAddress;
    for reply in [
        TaskTransitionReply32::Fault(TaskTransitionFault32::NotPresent(0)),
        TaskTransitionReply32::Fault(TaskTransitionFault32::GeneralProtection(4)),
        TaskTransitionReply32::Fault(TaskTransitionFault32::GeneralProtection(0)),
        TaskTransitionReply32::Fault(TaskTransitionFault32::StackSegment(0x50)),
        TaskTransitionReply32::Fault(TaskTransitionFault32::NotPresent(0x4c)),
        TaskTransitionReply32::Committed {
            fault: Some(TaskTransitionFault32::StackSegment(0)),
            code_invalidated: false,
            task_trap: false,
        },
        TaskTransitionReply32::Fault(TaskTransitionFault32::InvalidTss(0x4d)),
        TaskTransitionReply32::Authority(TaskAuthorityRequirement32::Privilege(4)),
        TaskTransitionReply32::Memory {
            fault: MemoryFault {
                address: GuestAddress(0xffff_ffff),
                access: Access::Read,
                reason: FaultReason::AddressOverflow,
            },
            length: 2,
        },
        TaskTransitionReply32::Memory {
            fault: MemoryFault {
                address: GuestAddress(0),
                access: Access::Execute,
                reason: FaultReason::Permission,
            },
            length: 2,
        },
        TaskTransitionReply32::Memory {
            fault: MemoryFault {
                address: GuestAddress(0),
                access: Access::Read,
                reason: FaultReason::Unmapped,
            },
            length: 103,
        },
        TaskTransitionReply32::Memory {
            fault: MemoryFault {
                address: GuestAddress(0),
                access: Access::Write,
                reason: FaultReason::Unmapped,
            },
            length: 104,
        },
    ] {
        let mut bytes = [0xa5; HELPER_SIZE];
        assert_eq!(
            encode_task_transition_result(reply, &mut bytes),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(bytes, [0xa5; HELPER_SIZE]);
    }
}
