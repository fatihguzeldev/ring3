use ring3_engine::{
    cpu::x86::{
        SegmentRegister,
        decode::decode_one,
        ir::{AddressSize, Operation},
        simd::{SimdKind, SimdOperand},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn fx_images_retain_full_operand_span_and_segment_selection() {
    for save in [true, false] {
        let extension = if save { 0 } else { 8 };
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
                vec![0x64, 0x0f, 0xae, 0x07 | extension],
                SegmentRegister::Fs,
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
            let Operation::Simd(operation) = decoded.operation() else {
                panic!("expected FX state image operation");
            };
            assert_eq!(
                operation.kind,
                if save {
                    SimdKind::SaveExtended
                } else {
                    SimdKind::RestoreExtended
                }
            );
            assert_eq!(operation.memory_bytes, 512);
            assert!(operation.aligned);
            let SimdOperand::Memory(address) = (if save {
                operation.destination
            } else {
                operation.source
            }) else {
                panic!("expected FX memory operand");
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
