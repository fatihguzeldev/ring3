use super::*;
use crate::cpu::x86::{SegmentDescriptorRecord, SegmentState};

fn record(
    kind: u8,
    dpl: u8,
    present: bool,
    limit: u32,
    granularity: bool,
    default32: bool,
    base: u32,
) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (limit & 0xffff) | (base << 16),
        (base & 0xff00_0000)
            | ((base >> 16) & 0xff)
            | (u32::from(kind) << 8)
            | (u32::from(dpl) << 13)
            | (u32::from(present) << 15)
            | (limit & 0xf0000)
            | (u32::from(granularity) << 23)
            | (u32::from(default32) << 22),
    )
}

#[test]
fn far_jump_outer_descriptors_check_every_type_privilege_presence_and_table_without_mutation() {
    for table_selector in [0x40_u16, 0x44] {
        for system in [false, true] {
            for kind in 0..16_u8 {
                for dpl in 0..4_u8 {
                    for rpl in 0..4_u16 {
                        for present in [false, true] {
                            let mut table = SegmentDescriptorTable::empty();
                            let raw = record(
                                kind | if system { 0 } else { 0x10 },
                                dpl,
                                present,
                                0xfffff,
                                true,
                                true,
                                0,
                            );
                            table.configure_raw(table_selector, raw).unwrap();
                            let before = table.clone();
                            let selector = table_selector | rpl;
                            let result =
                                table.resolve_far_jump(selector, 0x12345678, FarWidth::Dword, 3);
                            let expected = if system {
                                // Intel rev093 JMP TASK-STATE-SEGMENT/CALL-GATE/TASK-GATE:
                                // availability/table eligibility, outer privilege, then presence.
                                let eligible = [4, 5, 12].contains(&kind)
                                    || (table_selector == 0x40 && [1, 9].contains(&kind));
                                Err(if !eligible || dpl != 3 {
                                    CodeTransferError::GeneralProtection(u32::from(table_selector))
                                } else if !present {
                                    CodeTransferError::NotPresent(u32::from(table_selector))
                                } else {
                                    CodeTransferError::UnsupportedProfile
                                })
                            } else if kind & 8 == 0 || (kind & 4 == 0 && dpl != 3) {
                                Err(CodeTransferError::GeneralProtection(u32::from(
                                    table_selector,
                                )))
                            } else if !present {
                                Err(CodeTransferError::NotPresent(u32::from(table_selector)))
                            } else {
                                Ok(())
                            };
                            assert_eq!(
                                result.map(|_| ()),
                                expected,
                                "S0={system} type={kind} DPL={dpl} RPL={rpl} P={present}"
                            );
                            assert_eq!(table, before);
                            if let Ok(transfer) = result {
                                assert_eq!(transfer.selector(), table_selector | 3);
                                assert_eq!(transfer.offset, 0x12345678);
                                let mut context = SegmentContext::legacy(SegmentState::default());
                                transfer.apply(&mut context);
                                assert!(context.is_valid());
                                assert_eq!(
                                    context.caches[1].bits() >> 20 & 0xf,
                                    u32::from(kind | 1)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn normalized_offset_limit_and_execution_profile_refusals_are_distinct() {
    let mut table = SegmentDescriptorTable::empty();
    for (raw, width, offset, expected) in [
        (
            record(0x1a, 3, true, 0xffff, false, true, 0),
            FarWidth::Dword,
            0x10000,
            CodeTransferError::GeneralProtection(0),
        ),
        (
            record(0x1a, 3, true, 0xfffff, true, true, 0x1000),
            FarWidth::Dword,
            1,
            CodeTransferError::UnsupportedProfile,
        ),
        (
            record(0x1a, 3, true, 0xfffff, true, false, 0),
            FarWidth::Dword,
            1,
            CodeTransferError::UnsupportedProfile,
        ),
        (
            SegmentDescriptorRecord::from_words(0xffff, 0x00effa00),
            FarWidth::Dword,
            1,
            CodeTransferError::UnsupportedProfile,
        ),
    ] {
        table.configure_raw(0x40, raw).unwrap();
        assert_eq!(
            table.resolve_far_jump(0x43, offset, width, 3),
            Err(expected)
        );
        assert_eq!(*table.raw_descriptor(0x43).unwrap(), raw);
    }
    table
        .configure_raw(0x40, record(0x1a, 3, true, 0xffff, false, true, 0))
        .unwrap();
    let transfer = table
        .resolve_far_jump(0x40, 0xffff1234, FarWidth::Word, 3)
        .unwrap();
    assert_eq!((transfer.offset, transfer.selector()), (0x1234, 0x43));
    let mut context = SegmentContext::default();
    transfer.apply(&mut context);
    assert_eq!(context.caches[1].limit(), 0xffff);
    assert!(context.is_valid());
    for selector in [0, 1, 2, 3] {
        assert_eq!(
            table.resolve_far_jump(selector, 0, FarWidth::Dword, 3),
            Err(CodeTransferError::GeneralProtection(0))
        );
    }
    assert_eq!(
        table.resolve_far_jump(4, 0, FarWidth::Dword, 3),
        Err(CodeTransferError::GeneralProtection(4))
    );
    assert_eq!(
        table.resolve_far_jump(0x4b, 0, FarWidth::Dword, 3),
        Err(CodeTransferError::GeneralProtection(0x48))
    );
}

#[test]
fn known_invalid_system_metadata_is_distinct_from_unresolved_task_and_gate_payloads() {
    // expected results are taken directly from the JMP operation's three named branches.
    for (selector, kind, dpl, present, expected) in [
        (0x43, 3, 3, true, CodeTransferError::GeneralProtection(0x40)),
        (
            0x43,
            11,
            3,
            true,
            CodeTransferError::GeneralProtection(0x40),
        ),
        (0x47, 1, 3, true, CodeTransferError::GeneralProtection(0x44)),
        (0x47, 9, 3, true, CodeTransferError::GeneralProtection(0x44)),
        (0x43, 1, 2, true, CodeTransferError::GeneralProtection(0x40)),
        (0x43, 9, 3, false, CodeTransferError::NotPresent(0x40)),
        (0x47, 4, 2, true, CodeTransferError::GeneralProtection(0x44)),
        (0x47, 5, 2, true, CodeTransferError::GeneralProtection(0x44)),
        (
            0x47,
            12,
            2,
            true,
            CodeTransferError::GeneralProtection(0x44),
        ),
        (0x47, 4, 3, false, CodeTransferError::NotPresent(0x44)),
        (0x47, 5, 3, false, CodeTransferError::NotPresent(0x44)),
        (0x47, 12, 3, false, CodeTransferError::NotPresent(0x44)),
        (0x43, 1, 3, true, CodeTransferError::UnsupportedProfile),
        (0x43, 9, 3, true, CodeTransferError::UnsupportedProfile),
        (0x47, 4, 3, true, CodeTransferError::UnsupportedProfile),
        (0x47, 5, 3, true, CodeTransferError::UnsupportedProfile),
        (0x47, 12, 3, true, CodeTransferError::UnsupportedProfile),
    ] {
        let mut table = SegmentDescriptorTable::empty();
        table
            .configure_raw(selector, record(kind, dpl, present, 0, false, false, 0))
            .unwrap();
        let before = table.clone();
        assert_eq!(
            table.resolve_far_jump(selector, 0, FarWidth::Dword, 3),
            Err(expected)
        );
        assert_eq!(table, before);
    }
}

#[test]
fn call_descriptor_phase_defers_target_limit_until_after_stack_capacity() {
    let mut table = SegmentDescriptorTable::empty();
    let bounded = record(0x1a, 3, true, 0xffff, false, true, 0);
    table.configure_raw(0x40, bounded).unwrap();
    let destination = table.resolve_far_call(0x43, 3).unwrap();
    assert_eq!(
        destination.target(0x10000, FarWidth::Dword),
        Err(CodeTransferError::GeneralProtection(0))
    );
    assert_eq!(
        destination.target(0x1234, FarWidth::Word).unwrap().offset,
        0x1234
    );
    assert_eq!(*table.raw_descriptor(0x43).unwrap(), bounded);
}

#[test]
fn return_checks_selector_rpl_even_for_conforming_code_before_presence() {
    let mut table = SegmentDescriptorTable::empty();
    for (kind, dpl, present, rpl, expected) in [
        (
            0x1e,
            0,
            true,
            0,
            Err(CodeTransferError::GeneralProtection(0x40)),
        ),
        (
            0x1e,
            0,
            false,
            2,
            Err(CodeTransferError::GeneralProtection(0x40)),
        ),
        (0x1e, 0, true, 3, Ok(())),
        (0x1a, 3, true, 3, Ok(())),
        (
            0x1a,
            2,
            true,
            3,
            Err(CodeTransferError::GeneralProtection(0x40)),
        ),
        (0x1e, 3, false, 3, Err(CodeTransferError::NotPresent(0x40))),
        (
            0x12,
            3,
            true,
            3,
            Err(CodeTransferError::GeneralProtection(0x40)),
        ),
        (
            12,
            3,
            true,
            3,
            Err(CodeTransferError::GeneralProtection(0x40)),
        ),
    ] {
        let raw = record(kind, dpl, present, 0xfffff, true, true, 0);
        table.configure_raw(0x40, raw).unwrap();
        let before = table.clone();
        assert_eq!(
            table
                .resolve_far_return(0x40 | rpl, 0xffff1234, FarWidth::Word, 3)
                .map(|v| {
                    assert_eq!(v.offset, 0x1234);
                    assert_eq!(v.selector(), 0x43);
                }),
            expected
        );
        assert_eq!(table, before);
    }
    for selector in [0, 1, 2, 3] {
        assert_eq!(
            table.resolve_far_return(selector, 0, FarWidth::Dword, 3),
            Err(CodeTransferError::GeneralProtection(0))
        );
    }
    assert_eq!(
        table.resolve_far_return(0x4b, 0, FarWidth::Dword, 3),
        Err(CodeTransferError::GeneralProtection(0x48))
    );
}
