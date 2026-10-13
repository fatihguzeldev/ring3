use super::*;

fn gate(
    kind: u32,
    selector: u16,
    offset: u32,
    dpl: u32,
    present: bool,
    count: u32,
) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (u32::from(selector) << 16) | (offset & 0xffff),
        (if kind == 12 { offset & 0xffff0000 } else { 0 })
            | (kind << 8)
            | (dpl << 13)
            | (u32::from(present) << 15)
            | count,
    )
}

#[test]
fn selected_width_offset_and_target_rpl_are_independent_of_instruction_width() {
    for (kind, offset, selected) in [
        (4, 0x4321, FarWidth::Word),
        (12, 0x12344321, FarWidth::Dword),
    ] {
        for operand in [FarWidth::Word, FarWidth::Dword] {
            for count in [0, 31] {
                let mut table = SegmentDescriptorTable::empty();
                table
                    .configure_raw(0x40, gate(kind, 0x48, offset, 3, true, count))
                    .unwrap();
                let raw = SegmentDescriptorRecord::from_words(0xffff, 0x00cffa00);
                table.configure_raw(0x48, raw).unwrap();
                let before = table.clone();
                let destination = table.resolve_far_call(0x43, 3).unwrap();
                assert_eq!(destination.effective_width(operand), selected);
                let transfer = destination.target(0xdeadbeef, operand).unwrap();
                assert_eq!(
                    (transfer.offset, transfer.selector(), transfer.gate_width()),
                    (offset, 0x4b, Some(selected))
                );
                assert_eq!(
                    table
                        .resolve_far_jump(0x43, 0xdeadbeef, operand, 3)
                        .unwrap(),
                    transfer
                );
                assert_eq!(
                    table, before,
                    "pure resolution does not set target accessed or change gate"
                );
            }
        }
    }
}

#[test]
fn inner_type_and_privilege_precede_presence_and_privilege_change_stays_explicit() {
    for (high, call, expected) in [
        (0x00cf7200, true, CodeTransferError::GeneralProtection(0x48)),
        (
            0x00cf9a00,
            false,
            CodeTransferError::GeneralProtection(0x48),
        ),
        (0x00cf1a00, true, CodeTransferError::NotPresent(0x48)),
        (0x00cf9a00, true, CodeTransferError::UnsupportedProfile),
    ] {
        let mut table = SegmentDescriptorTable::empty();
        table
            .configure_raw(0x40, gate(12, 0x4b, 0x1234, 3, true, 31))
            .unwrap();
        table
            .configure_raw(0x48, SegmentDescriptorRecord::from_words(0xffff, high))
            .unwrap();
        let before = table.clone();
        assert_eq!(table.destination(0x43, 3, call), Err(expected));
        assert_eq!(table, before);
    }
    // a lower-DPL conforming segment keeps the caller's CPL, regardless target selector RPL.
    let mut table = SegmentDescriptorTable::empty();
    table
        .configure_raw(0x40, gate(12, 0x48, 0x1234, 3, true, 31))
        .unwrap();
    table
        .configure_raw(
            0x48,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cf9e00),
        )
        .unwrap();
    for call in [false, true] {
        let transfer = table
            .destination(0x43, 3, call)
            .unwrap()
            .target(0, FarWidth::Word)
            .unwrap();
        assert_eq!((transfer.offset, transfer.selector()), (0x1234, 0x4b));
    }
}

#[test]
fn outer_faults_precede_unresolved_inner_selector_and_reserved_payload_boundary() {
    for (kind, dpl, present, reserved, expected) in [
        (
            12,
            2,
            false,
            0x80,
            CodeTransferError::GeneralProtection(0x40),
        ),
        (12, 3, false, 0x80, CodeTransferError::NotPresent(0x40)),
        (12, 3, true, 0x80, CodeTransferError::UnsupportedProfile),
        (4, 3, true, 0x10000, CodeTransferError::UnsupportedProfile),
        (12, 3, true, 0, CodeTransferError::GeneralProtection(0)),
        (4, 3, true, 0, CodeTransferError::GeneralProtection(0)),
    ] {
        let mut table = SegmentDescriptorTable::empty();
        let raw = gate(kind, 0, 0, dpl, present, 0);
        let (low, high) = raw.to_words();
        table
            .configure_raw(
                0x40,
                SegmentDescriptorRecord::from_words(low, high | reserved),
            )
            .unwrap();
        let before = table.clone();
        assert_eq!(
            table.resolve_far_jump(0x43, 0, FarWidth::Dword, 3),
            Err(expected)
        );
        assert_eq!(table, before);
    }
}
