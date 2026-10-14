use super::*;
use crate::cpu::x86::{DebugCause32, EventOrigin32, ExceptionCause32, SegmentDescriptorRecord};

fn table(vector: u8, kind: u8, present: bool) -> InterruptDescriptorTable32 {
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    records[usize::from(vector)] = SegmentDescriptorRecord::from_words(
        0x00402345,
        (u32::from(kind) << 8) | (u32::from(present) << 15),
    );
    InterruptDescriptorTable32::new(0xffff, records)
}

#[test]
fn event_origin_bypasses_only_outer_gate_dpl_and_derives_secondary_ext() {
    for (origin, vector) in [
        (EventOrigin32::SoftwareInt { vector: 3 }, 3),
        (
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
            3,
        ),
        (EventOrigin32::Exception(ExceptionCause32::OverflowInto), 4),
    ] {
        assert_eq!(
            table(vector, 14, false).resolve_event_gate(origin, 3),
            Err(CodeTransferError::GeneralProtection(
                (u32::from(vector) << 3) | 2
            ))
        );
    }
    for (origin, vector) in [
        (EventOrigin32::Exception(ExceptionCause32::InvalidOpcode), 6),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
            1,
        ),
    ] {
        assert_eq!(
            table(vector, 14, false).resolve_event_gate(origin, 3),
            Err(CodeTransferError::NotPresent((u32::from(vector) << 3) | 3))
        );
        let selected = table(vector, 14, true)
            .resolve_event_gate(origin, 3)
            .unwrap();
        let mut descriptors = SegmentDescriptorTable::empty();
        descriptors
            .configure_raw(
                0x40,
                SegmentDescriptorRecord::from_words(0xffff, 0x00cf1a00),
            )
            .unwrap();
        assert_eq!(
            selected.event_destination(&descriptors, 3, origin),
            Err(CodeTransferError::NotPresent(0x41))
        );
        descriptors
            .configure_raw(
                0x40,
                SegmentDescriptorRecord::from_words(0xffff, 0x00cf9a00),
            )
            .unwrap();
        assert_eq!(
            selected.event_destination(&descriptors, 3, origin),
            Err(CodeTransferError::UnsupportedProfile),
            "bypass never changes current privilege for target admission"
        );
    }
}
