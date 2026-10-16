use super::{support_fetch as fetch, support_prefix_admission as admission};
use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
};

fn compiler_nop(length: usize) -> Vec<u8> {
    assert!((11..=15).contains(&length));
    // llvm and gas extend their ten-byte cs nop with leading operand-size prefixes.
    [
        vec![0x66; length - 9],
        vec![0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0],
    ]
    .concat()
}

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

#[test]
fn compiler_multi66_alignment_keeps_one_complete_instruction_and_all_source_bytes() {
    for length in 11..=15 {
        let bytes = compiler_nop(length);
        let pc = 0x2000 - length as u32;
        let mut memory = memory(pc, &bytes);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::Nop);
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (length, GuestAddress(0x2000))
        );
        for offset in 0..length {
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert!(memory.is_code_current(decoded.code_snapshot()));
            memory
                .write(GuestAddress(pc + offset as u32), &bytes[offset..offset + 1])
                .unwrap();
            assert!(
                !memory.is_code_current(decoded.code_snapshot()),
                "source byte {offset} must remain in identity"
            );
        }
        assert_eq!(admission::admitted(&bytes), (Operation::Nop, length));
    }
}

#[test]
fn compiler_multi66_alignment_fetches_every_displacement_byte_before_admission() {
    for length in 11..=15 {
        let bytes = compiler_nop(length);
        for cut in 1..length {
            let pc = 0x2000 - cut as u32;
            let mut memory = memory(pc, &bytes[..cut]);
            fetch::assert_error(
                decode_one(&memory, GuestAddress(pc)).unwrap_err(),
                pc,
                0x2000,
                FaultReason::Unmapped,
                cut as u32 + 1,
            );
            let next = PageRange::new(GuestAddress(0x2000), 1).unwrap();
            memory.map_zeroed(next, Permissions::READ).unwrap();
            fetch::assert_error(
                decode_one(&memory, GuestAddress(pc)).unwrap_err(),
                pc,
                0x2000,
                FaultReason::Permission,
                cut as u32 + 1,
            );
        }
    }
}

#[test]
fn compiler_alignment_exception_keeps_other_duplicate_and_reserved_prefix_boundaries() {
    for bytes in [
        &[0x66, 0x66, 0x90][..],
        &[0x66, 0x66, 0x89, 0xc0],
        &[0x66, 0x66, 0xc3],
        &[0x66, 0x66, 0x0f, 0x19, 0],
        &[0x66, 0x66, 0x0f, 0x1f, 0x08],
        &[0xf2, 0x66, 0x66, 0x0f, 0x1f, 0],
        &[0xf3, 0x66, 0x66, 0x0f, 0x1f, 0],
    ] {
        crate::support_fetch::assert_decode_contract(
            decode_one(&memory(0x1000, bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        );
    }
    let locked = [vec![0xf0], compiler_nop(11)].concat();
    crate::support_fetch::assert_decode_contract(
        decode_one(&memory(0x1000, &locked), GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding,
    );
    let overlong = [vec![0x66], compiler_nop(15)].concat();
    crate::support_fetch::assert_decode_contract(
        decode_one(&memory(0x1000, &overlong), GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding,
    );
}
