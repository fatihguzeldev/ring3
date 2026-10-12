use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    Ok(*instruction.operation())
}

#[test]
fn ignored_operand_size_on_byte_operations_keeps_the_decoded_byte_semantics() {
    for bytes in [
        &[0xb0, 0x81][..],
        &[0x00, 0xd0][..],
        &[0x0f, 0x94, 0xc0][..],
    ] {
        let original = decode(bytes).unwrap();
        let mut prefixed = vec![0x66];
        prefixed.extend_from_slice(bytes);
        assert_eq!(decode(&prefixed).unwrap(), original);
    }
}

#[test]
fn repeated_prefix_groups_and_non_string_rep_remain_explicit_refusals() {
    for bytes in [
        &[0x66, 0x66, 0xb0, 1][..],
        &[0xf3, 0xf3, 0x0f, 0xbc, 0xc0][..],
        &[0xf2, 0xf3, 0x0f, 0xbd, 0xc0][..],
        &[0xf3, 0x00, 0xd0][..],
    ] {
        assert!(decode(bytes).is_err(), "{bytes:02x?}");
    }
}
