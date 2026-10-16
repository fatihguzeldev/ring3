use super::*;
#[test]
fn x86_rpc_message_preserves_opaque_representation_flags_and_reserved_words() {
    let literal = [
        0x11, 0, 0, 0, 0x10, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 8, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0,
        2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0xa5,
    ];
    let message = RpcMessage32::decode(&literal).unwrap();
    assert_eq!(message.reserved, 0x11);
    assert_eq!(message.representation, 0x10);
    assert_eq!(message.buffer, 0x12345678);
    assert_eq!(message.length, 8);
    assert_eq!(message.method, 7);
    assert_eq!(message.reserved2, [1, 2, 3, 4, 5]);
    assert_eq!(message.flags, 0xa5000000);
    assert_eq!(message.encode(), literal);
    let changed = RpcMessage32 {
        buffer: 0xfedcba98,
        length: 16,
        ..message
    }
    .encode();
    assert_eq!(&changed[..8], &literal[..8]);
    assert_eq!(&changed[16..], &literal[16..]);
    assert_eq!(&changed[8..16], &[0x98, 0xba, 0xdc, 0xfe, 16, 0, 0, 0]);
}
#[test]
fn rpc_message_extent_is_exact_and_never_reads_a_partial_field() {
    for length in 0..RpcMessage32::SIZE {
        assert!(RpcMessage32::decode(&[0u8; 44][..length]).is_none());
    }
    assert!(RpcMessage32::decode(&[0; 45]).is_none());
    assert_eq!(
        RpcMessage32::decode(&[0; 44]),
        Some(RpcMessage32::default())
    );
}
