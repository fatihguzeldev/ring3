use super::*;
use crate::{
    cpu::x86::{EnteredEvent32, SelectorErrorCode32},
    memory::{GuestAddress, MemoryError, MemoryFault},
};

fn literal(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 12, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + 4 * index..20 + 4 * index].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn gp(code: u32) -> ExceptionCause32 {
    ExceptionCause32::GeneralProtection(SelectorErrorCode32::from_bits(code).unwrap())
}

#[test]
fn exception_frames_have_exact_width_error_and_gate_metadata() {
    for (origin, info, target) in [
        (
            EventOrigin32::Exception(ExceptionCause32::InvalidOpcode),
            2,
            0xfffe,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::InvalidOpcode),
            0x104,
            0x12345678,
        ),
        (EventOrigin32::Exception(gp(0)), 0x202, 0xffff),
        (EventOrigin32::Exception(gp(0x48)), 0x304, u32::MAX),
    ] {
        let entered = EventDeliveryResult32::Entered(EnteredEvent32 {
            target,
            esp: 0xfffffff0,
            flags: 2,
            frame_info: info,
            code_invalidated: true,
        });
        let mut packet = [0xa5; 40];
        encode_exception_result(origin, info, &entered, &mut packet).unwrap();
        assert_eq!(packet, literal([0, target, 0, 0, 0, info]));
    }
}

#[test]
fn nested_idt_selector_and_stack_errors_preserve_the_origin_ext_bit() {
    for (origin, info, cause, expected) in [
        (
            EventOrigin32::SoftwareInt { vector: 3 },
            0,
            gp(0x1a),
            [3, 0, 0x1a, 0, 0, 0],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::InvalidOpcode),
            0,
            gp(0x33),
            [3, 0, 0x33, 0, 0, 0],
        ),
        (
            EventOrigin32::Exception(gp(0)),
            0x304,
            ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::descriptor(0x4b, true)),
            [6, 0, 0x49, 0, 0, 0x304],
        ),
        (
            EventOrigin32::Exception(gp(0)),
            0x204,
            ExceptionCause32::StackSegment(SelectorErrorCode32::null(true)),
            [7, 0, 1, 0, 0, 0x204],
        ),
    ] {
        let mut packet = [0xa5; 40];
        encode_exception_result(
            origin,
            info,
            &EventDeliveryResult32::ArchitecturalSecondary(cause),
            &mut packet,
        )
        .unwrap();
        assert_eq!(packet, literal(expected));
    }
}

#[test]
fn missing_authorities_and_memory_diagnostics_do_not_become_architectural_page_faults() {
    let origin = EventOrigin32::Exception(gp(0));
    for (info, result, expected) in [
        (
            0,
            EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                kind: EventAuthorityKind32::Task,
                selector: 0x48,
            },
            [8, 0, 1, 0x48, 0, 0],
        ),
        (
            0x304,
            EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                kind: EventAuthorityKind32::Privilege,
                selector: 0x4b,
            },
            [8, 0, 2, 0x4b, 0, 0x304],
        ),
        (
            0x204,
            EventDeliveryResult32::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(0),
                    access: Access::Write,
                    reason: FaultReason::Unmapped,
                },
                length: 4,
            },
            [1, 0, 1, 0, 2, 0x204],
        ),
        (
            0x202,
            EventDeliveryResult32::Infrastructure(MemoryError::VersionExhausted),
            [2, 0, 1, 0, 0, 0x202],
        ),
    ] {
        let mut packet = [0xa5; 40];
        encode_exception_result(origin, info, &result, &mut packet).unwrap();
        assert_eq!(packet, literal(expected));
    }
}

#[test]
fn malformed_phase_metadata_and_error_codes_leave_output_untouched() {
    let origin = EventOrigin32::Exception(gp(0));
    for (info, result) in [
        (1, EventDeliveryResult32::ArchitecturalSecondary(gp(1))),
        (0x404, EventDeliveryResult32::ArchitecturalSecondary(gp(1))),
        (4, EventDeliveryResult32::ArchitecturalSecondary(gp(1))),
        (0, EventDeliveryResult32::ArchitecturalSecondary(gp(0x6a))),
        (
            0x204,
            EventDeliveryResult32::ArchitecturalSecondary(gp(0x48)),
        ),
        (
            0x204,
            EventDeliveryResult32::ArchitecturalSecondary(gp(0x4b)),
        ),
        (
            0x204,
            EventDeliveryResult32::ArchitecturalSecondary(ExceptionCause32::SegmentNotPresent(
                SelectorErrorCode32::null(true),
            )),
        ),
        (
            0x204,
            EventDeliveryResult32::ArchitecturalSecondary(ExceptionCause32::StackSegment(
                SelectorErrorCode32::descriptor(0x48, true),
            )),
        ),
        (
            0x204,
            EventDeliveryResult32::ArchitecturalSecondary(ExceptionCause32::DivideError),
        ),
        (
            0x204,
            EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                kind: EventAuthorityKind32::Task,
                selector: 0x48,
            },
        ),
        (
            0,
            EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                kind: EventAuthorityKind32::Privilege,
                selector: 0x48,
            },
        ),
        (
            0x202,
            EventDeliveryResult32::Entered(EnteredEvent32 {
                target: 0x10000,
                esp: 0,
                flags: 2,
                frame_info: 0x202,
                code_invalidated: false,
            }),
        ),
        (
            0x204,
            EventDeliveryResult32::Entered(EnteredEvent32 {
                target: 0,
                esp: 0,
                flags: 2,
                frame_info: 0x304,
                code_invalidated: false,
            }),
        ),
        (
            0x204,
            EventDeliveryResult32::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(0),
                    access: Access::Read,
                    reason: FaultReason::Unmapped,
                },
                length: 4,
            },
        ),
        (
            0x204,
            EventDeliveryResult32::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(0),
                    access: Access::Write,
                    reason: FaultReason::AddressOverflow,
                },
                length: 4,
            },
        ),
        (
            0x204,
            EventDeliveryResult32::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(0),
                    access: Access::Write,
                    reason: FaultReason::Permission,
                },
                length: 2,
            },
        ),
    ] {
        let mut packet = [0xa5; 40];
        assert!(
            encode_exception_result(origin, info, &result, &mut packet).is_err(),
            "{info:x} {result:?}"
        );
        assert_eq!(packet, [0xa5; 40]);
    }
    let mut short = [0xa5; 39];
    assert_eq!(
        encode_exception_result(
            origin,
            0,
            &EventDeliveryResult32::ArchitecturalSecondary(gp(0x6b)),
            &mut short
        ),
        Err(AbiError::Length)
    );
    assert_eq!(short, [0xa5; 39]);
}

#[test]
fn old_software_interrupt_encoder_cannot_accept_exception_metadata_or_ext_errors() {
    use crate::abi::interrupt::{InterruptHelperError, encode_interrupt_result};
    for (info, code) in [(0, 0x33), (4, 1), (0x204, 0)] {
        let mut packet = [0xa5; 40];
        assert!(
            encode_interrupt_result(
                6,
                info,
                0,
                Err(InterruptHelperError::Cpu(
                    CpuMemoryError::GeneralProtection(code)
                )),
                &mut packet
            )
            .is_err()
        );
        assert_eq!(packet, [0xa5; 40]);
    }
}
