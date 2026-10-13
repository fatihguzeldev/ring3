use super::*;

#[test]
fn unchecked_scalar_base_identity_controls_default_segment() {
    for (base, expected) in [
        (None, SegmentRegister::Ds),
        (Some(Register32::Eax), SegmentRegister::Ds),
        (Some(Register32::Ebp), SegmentRegister::Ss),
        (Some(Register32::Esp), SegmentRegister::Ss),
    ] {
        let address = GatherAddress::new(base, u32::MAX, None);
        assert_eq!(address.base(), base);
        assert_eq!(address.displacement(), u32::MAX);
        assert_eq!(address.resolved_segment(), expected);
        let override_address = GatherAddress::new(base, 0x20, Some(SegmentRegister::Fs));
        assert_eq!(override_address.resolved_segment(), SegmentRegister::Fs);
    }
}
