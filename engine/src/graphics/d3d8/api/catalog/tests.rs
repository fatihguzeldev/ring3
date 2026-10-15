use super::*;

#[test]
fn all_interface_slots_have_distinct_roundtripping_gates_and_bounded_frames() {
    let counts = [16, 97, 11, 14, 19, 19, 19, 14, 14, 11, 11, 5];
    let mut ids = std::collections::BTreeSet::new();
    for (interface, count) in InterfaceKind::ALL.into_iter().zip(counts) {
        assert_eq!(interface.methods().len(), count);
        for (slot, method) in interface.methods().iter().enumerate() {
            assert!((1..=16).contains(&method.stack_words));
            let gate = MethodGate {
                interface,
                slot: slot as u8,
            };
            assert!(ids.insert(gate.id()));
            assert_eq!(MethodGate::from_id(gate.id()), Some(gate));
        }
        assert!(
            MethodGate::from_id(
                MethodGate {
                    interface,
                    slot: count as u8
                }
                .id()
            )
            .is_none()
        );
    }
    assert_eq!(ids.len(), 250);
    for id in [0, CREATE_GATE, 0x0004_0000, 0x0004_ffff, 0x0005_0200] {
        assert!(MethodGate::from_id(id).is_none());
    }
}

#[test]
fn canonical_method_positions_and_iid_byte_order_match_the_guest_abi() {
    let root = InterfaceKind::Direct3D.methods();
    assert_eq!((root[15].name, root[15].stack_words), ("CreateDevice", 7));
    let device = InterfaceKind::Device.methods();
    assert_eq!(
        (device[20].name, device[20].stack_words),
        ("CreateTexture", 8)
    );
    assert_eq!((device[36].name, device[36].stack_words), ("Clear", 7));
    assert_eq!(device[96].name, "DeletePatch");
    assert_eq!(
        InterfaceKind::Direct3D.iid().0,
        [
            0xda, 0xe8, 0xd9, 0x1d, 0x77, 0x1c, 0x40, 0x4d, 0xb0, 0xcf, 0x98, 0xfe, 0xfd, 0xff,
            0x95, 0x12
        ]
    );
    assert!(InterfaceKind::Texture.supports(InterfaceKind::Resource));
    assert!(!InterfaceKind::Surface.supports(InterfaceKind::Resource));
}
