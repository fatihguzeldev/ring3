use super::*;
use crate::cpu::x86::{SegmentContext, SegmentDescriptorRecord};

const VECTOR: u8 = 0x80;
fn gate(kind: u8, selector: u16, offset: u32, dpl: u8, present: bool) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (u32::from(selector) << 16) | (offset & 0xffff),
        (if matches!(kind, 14 | 15) {
            offset & 0xffff0000
        } else {
            0
        }) | (u32::from(kind) << 8)
            | (u32::from(dpl) << 13)
            | (u32::from(present) << 15),
    )
}
fn idt(limit: u16, record: SegmentDescriptorRecord) -> InterruptDescriptorTable32 {
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    records[usize::from(VECTOR)] = record;
    InterruptDescriptorTable32::new(limit, records)
}

#[test]
fn idt_limit_type_dpl_and_presence_have_ordered_vector_error_codes() {
    let error = (u32::from(VECTOR) << 3) | 2;
    for (limit, record, expected) in [
        (
            0x406,
            gate(14, 0, 0, 3, false),
            CodeTransferError::GeneralProtection(error),
        ),
        (
            0x407,
            gate(0, 0, 0, 0, false),
            CodeTransferError::GeneralProtection(error),
        ),
        (
            0x407,
            gate(14, 0, 0, 2, false),
            CodeTransferError::GeneralProtection(error),
        ),
        (
            0x407,
            gate(14, 0, 0, 3, false),
            CodeTransferError::NotPresent(error),
        ),
        (
            0x407,
            gate(5, 0, 0, 3, true),
            CodeTransferError::UnsupportedProfile,
        ),
    ] {
        let table = idt(limit, record);
        let original = *table.record(VECTOR);
        assert_eq!(table.resolve_gate(VECTOR, 3), Err(expected));
        assert_eq!(*table.record(VECTOR), original);
    }
    let mut bytes = gate(14, 0, 0, 3, false).to_bytes();
    bytes[5] |= 0x10;
    assert_eq!(
        idt(0x407, SegmentDescriptorRecord::from_bytes(bytes)).resolve_gate(VECTOR, 3),
        Err(CodeTransferError::GeneralProtection(error)),
        "S=1 is not an IDT gate before presence"
    );
}

#[test]
fn gate_width_and_kind_are_payload_properties_and_reserved_payloads_stay_neutral() {
    for (kind, width, frame, offset) in [
        (6, FarWidth::Word, 0x102, 0x2345),
        (7, FarWidth::Word, 2, 0x2345),
        (14, FarWidth::Dword, 0x104, 0xabcd2345),
        (15, FarWidth::Dword, 4, 0xabcd2345),
    ] {
        let table = idt(0x407, gate(kind, 0x40, offset, 3, true));
        let selected = table.resolve_gate(VECTOR, 3).unwrap();
        assert_eq!(
            (selected.width(), selected.frame_info(), selected.offset()),
            (width, frame, offset)
        );
    }
    for (kind, high_bits) in [(6, 1), (7, 0x10000), (14, 0x80), (15, 0xff)] {
        let record = gate(kind, 0x40, 0, 3, true);
        let (low, high) = record.to_words();
        let table = idt(
            0x407,
            SegmentDescriptorRecord::from_words(low, high | high_bits),
        );
        assert_eq!(
            table.resolve_gate(VECTOR, 3),
            Err(CodeTransferError::UnsupportedProfile)
        );
    }
    let record = gate(6, 0, 0, 3, false);
    let (low, high) = record.to_words();
    assert_eq!(
        idt(0x407, SegmentDescriptorRecord::from_words(low, high | 1)).resolve_gate(VECTOR, 3),
        Err(CodeTransferError::NotPresent(0x402)),
        "outer presence precedes reserved payload policy"
    );
}

#[test]
fn embedded_rpl_is_ignored_while_type_presence_and_privilege_change_remain_distinct() {
    let mut descriptors = SegmentDescriptorTable::empty();
    for (raw, expected) in [
        (0x00cf7200, Some(CodeTransferError::GeneralProtection(0x40))),
        (0x00cf1a00, Some(CodeTransferError::NotPresent(0x40))),
        (0x00cf9a00, Some(CodeTransferError::UnsupportedProfile)),
        (0x00cf9e00, None),
        (0x00cffa00, None),
    ] {
        descriptors
            .configure_raw(0x40, SegmentDescriptorRecord::from_words(0xffff, raw))
            .unwrap();
        let before = descriptors.clone();
        for rpl in 0..4 {
            let selected = idt(0x407, gate(15, 0x40 | rpl, 0x1234, 3, true))
                .resolve_gate(VECTOR, 3)
                .unwrap();
            let result = selected.destination(&descriptors, 3);
            if let Some(error) = expected {
                assert_eq!(result, Err(error));
            } else {
                let transfer = result
                    .unwrap()
                    .target(selected.offset(), selected.width())
                    .unwrap();
                assert_eq!((transfer.offset, transfer.selector()), (0x1234, 0x43));
                let mut context = SegmentContext::default();
                transfer.apply(&mut context);
                assert_eq!(context.state.selectors[1], 0x43);
            }
        }
        assert_eq!(
            descriptors, before,
            "resolution never commits accessed or loaded CS"
        );
    }
    let null = idt(0x407, gate(15, 3, 0, 3, true))
        .resolve_gate(VECTOR, 3)
        .unwrap();
    assert_eq!(
        null.destination(&descriptors, 3),
        Err(CodeTransferError::GeneralProtection(0))
    );
}

#[test]
fn target_offset_limit_is_deferred_until_after_descriptor_admission() {
    let mut descriptors = SegmentDescriptorTable::empty();
    descriptors
        .configure_raw(0x40, SegmentDescriptorRecord::from_words(0xff, 0x0040fa00))
        .unwrap();
    let selected = idt(0x407, gate(14, 0x40, 0x100, 3, true))
        .resolve_gate(VECTOR, 3)
        .unwrap();
    let destination = selected.destination(&descriptors, 3).unwrap();
    assert_eq!(
        destination.target(selected.offset(), selected.width()),
        Err(CodeTransferError::GeneralProtection(0))
    );
}
