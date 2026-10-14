use super::*;
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{X87Format, X87State},
};
#[test]
fn waiting_x87_fault_precedes_memory_access_but_operand_pf_updates_cr2_before_fp_publication() {
    let mut e = EngineInstance::new(2, 7).unwrap();

    e.map(0x1000, 1, 7).unwrap();

    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xeb, 0xfe])
        .unwrap();
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    e.compile(1).unwrap();
    let mut state = X87State::default();
    state.control &= !1;
    state.status = 1;
    encode_x87(
        &state,
        &mut e.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    e.x87_op(
        X87Operation::Load(X87Format::Float32).encode(),
        0x5000,
        2,
        0x1000,
        0,
        0x401,
    )
    .unwrap();
    assert_eq!(e.cr2(), None);
    state = X87State::default();
    encode_x87(
        &state,
        &mut e.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    let before = e.arena().to_vec();
    e.x87_op(
        X87Operation::Load(X87Format::Float32).encode(),
        0x5000,
        2,
        0x1000,
        0,
        0x401,
    )
    .unwrap();
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
    // helper packet is root-owned; every other arena byte is unchanged.
    assert_eq!(&e.arena()[..HELPER_OFFSET], &before[..HELPER_OFFSET]);
    assert_eq!(
        &e.arena()[HELPER_OFFSET + HELPER_SIZE..],
        &before[HELPER_OFFSET + HELPER_SIZE..]
    );
}
