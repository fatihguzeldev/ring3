use ring3_engine::{
    cpu::x86::{
        SegmentRegister,
        decode::decode_one,
        ir::{AddressSize, Operation},
        xstate::XStateOperation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn xstate_selected_span_guards_keep_the_original_logical_address_and_segment() {
    for save in [true, false] {
        let extension = if save { 0x20 } else { 0x28 };
        for (bytes, segment, address16) in [
            (
                vec![0x0f, 0xae, 0x07 | extension],
                SegmentRegister::Ds,
                false,
            ),
            (
                vec![0x0f, 0xae, 0x45 | extension, 0],
                SegmentRegister::Ss,
                false,
            ),
            (
                vec![0x36, 0x0f, 0xae, 0x07 | extension],
                SegmentRegister::Ss,
                false,
            ),
            (
                vec![0x64, 0x0f, 0xae, 0x07 | extension],
                SegmentRegister::Fs,
                false,
            ),
            (
                vec![0x65, 0x0f, 0xae, 0x07 | extension],
                SegmentRegister::Gs,
                false,
            ),
            (
                vec![0x67, 0x0f, 0xae, 0x05 | extension],
                SegmentRegister::Ds,
                true,
            ),
        ] {
            let mut memory = AddressSpace::new(1).unwrap();
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
            let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            let Operation::XState(operation) = decoded.operation() else {
                panic!("expected xstate");
            };
            let address = match operation {
                XStateOperation::Save { address } if save => address,
                XStateOperation::Restore { address } if !save => address,
                _ => panic!("wrong state operation"),
            };
            assert_eq!(address.resolved_segment(), segment);
            assert_eq!(
                address.address_size,
                if address16 {
                    AddressSize::Bits16
                } else {
                    AddressSize::Bits32
                }
            );
            assert_eq!(usize::from(decoded.length()), bytes.len());
        }
    }
}
