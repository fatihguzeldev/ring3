use super::super::*;
use crate::{
    cpu::x86::{
        SegmentContext, UserSystemRegisterBank32,
        decode::{DecodeAuthority, DecodeError},
        ir::{Operation, Value32},
    },
    memory::{PageRange, Permissions},
};
fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}
fn cs() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}
#[test]
fn configured_interrupt_is_one_terminator_and_strict_extents_keep_their_end_contract() {
    let memory = memory(&[0xcd, 0x80, 0xf0, 0x90]);
    let authority = DecodeAuthority::new(None, true);
    let limits = CompileLimits::default();
    let prepared = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1000)],
        limits,
        &[],
        Some(cs()),
        authority,
    )
    .unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (1, 1)
    );
    let decoded = &prepared.blocks[0].instructions[0];
    assert_eq!(
        (decoded.operation(), decoded.length(), decoded.next_pc()),
        (
            &Operation::SoftwareInterrupt { vector: 0x80 },
            2,
            GuestAddress(0x1002)
        )
    );
    assert!(region::terminates(decoded, true));
    assert!(region::supports(decoded, true));
    assert!(!region::supports(decoded, false));
    let spec = BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: 2,
    };
    region::prepare_embedded_region(&memory, &[spec], limits, &[], Some(cs()), authority).unwrap();
    let bad = BlockSpec {
        byte_length: 4,
        ..spec
    };
    assert!(matches!(
        region::prepare_embedded_region(&memory, &[bad], limits, &[], Some(cs()), authority),
        Err(CompileError::Instruction {
            cause: InstructionError::InvalidBlockEnd,
            ..
        })
    ));
    assert!(prepare_entry_region(&memory, &[GuestAddress(0x1000)], limits).is_err());
}
#[test]
fn both_immutable_authorities_propagate_without_query_idtr_becoming_delivery_authority() {
    let memory = memory(&[0x0f, 0x00, 0xc0, 0xcd, 0x80]);
    let limits = CompileLimits::default();
    let bank = UserSystemRegisterBank32 {
        ldtr: Some(0x40),
        ..Default::default()
    };
    let prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        limits,
        Some(cs()),
        DecodeAuthority::new(Some(bank), true),
    )
    .unwrap();
    assert_eq!(prepared.instruction_count(), 2);
    assert!(matches!(
        prepared.blocks[0].instructions[0].operation(),
        Operation::Move {
            source: Value32::Immediate(0x40),
            ..
        }
    ));
    assert_eq!(
        prepared.blocks[0].instructions[1].operation(),
        &Operation::SoftwareInterrupt { vector: 0x80 }
    );
    assert!(matches!(
        prepare_runtime_entry(
            &memory,
            GuestAddress(0x1000),
            limits,
            Some(cs()),
            DecodeAuthority::new(None, true)
        ),
        Err(CompileError::Instruction {
            cause: InstructionError::Decode(DecodeError::MissingSystemRegisterAuthority(_)),
            ..
        })
    ));
    let without_idt = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1000)],
        limits,
        &[],
        Some(cs()),
        DecodeAuthority::new(Some(bank), false),
    );
    assert!(matches!(
        without_idt,
        Err(CompileError::Instruction {
            pc: GuestAddress(0x1003),
            cause: InstructionError::Decode(DecodeError::Unsupported(_))
        })
    ));
}
