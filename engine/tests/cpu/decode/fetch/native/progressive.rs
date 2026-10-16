use ring3_engine::{
    cpu::x86::{
        Register32,
        decode::decode_one,
        ir::{Operation, WordValue},
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
};

fn top_code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0, 0xffff_f000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(GuestAddress(0), &[0x34, 0x12, 0x56, 0x78])
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    for address in [0, 0xffff_f000] {
        memory
            .protect(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::EXECUTE,
            )
            .unwrap();
    }
    let mut completion = [0; 4];
    memory.fetch(GuestAddress(0), &mut completion).unwrap();
    assert_eq!(completion, [0x34, 0x12, 0x56, 0x78]);
    assert_eq!(memory.mapped_pages(), 2);
    memory
}

#[test]
fn incomplete_top_dword_move_faults_at_original_pc_despite_mapped_zero_completion() {
    let pc = GuestAddress(u32::MAX);
    let memory = top_code(pc.0, &[0xb8]);
    crate::support_fetch::assert_error(
        (decode_one(&memory, pc)).expect_err("code fetch must fail"),
        pc.0,
        pc.0,
        FaultReason::AddressOverflow,
        2,
    );
}

#[test]
fn incomplete_top_word_move_keeps_original_pc_and_progressive_fault_length() {
    let pc = GuestAddress(u32::MAX - 1);
    let memory = top_code(pc.0, &[0x66, 0xb8]);
    crate::support_fetch::assert_error(
        (decode_one(&memory, pc)).expect_err("code fetch must fail"),
        pc.0,
        pc.0,
        FaultReason::AddressOverflow,
        3,
    );
}

#[test]
fn completed_top_word_move_wraps_only_next_pc_and_keeps_current_snapshot() {
    let pc = GuestAddress(u32::MAX - 3);
    let memory = top_code(pc.0, &[0x66, 0xb8, 0x34, 0x12]);
    let decoded = decode_one(&memory, pc).unwrap();
    assert_eq!(decoded.pc(), pc);
    assert_eq!((decoded.length(), decoded.next_pc()), (4, GuestAddress(0)));
    assert_eq!(
        decoded.operation(),
        &Operation::MoveWord {
            destination: Register32::Eax,
            source: WordValue::Immediate(0x1234),
        }
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
}
