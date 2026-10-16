use crate::windows::com::InterfaceId;

const fn ole_iid(first: u32) -> InterfaceId {
    let mut bytes = [0; 16];
    let first = first.to_le_bytes();
    bytes[0] = first[0];
    bytes[1] = first[1];
    bytes[2] = first[2];
    bytes[3] = first[3];
    bytes[8] = 0xc0;
    bytes[15] = 0x46;
    InterfaceId(bytes)
}
pub(crate) const IID_IMARSHAL: InterfaceId = ole_iid(3);
pub(crate) const IID_ISTREAM: InterfaceId = ole_iid(12);
pub(crate) const IID_ISEQUENTIALSTREAM: InterfaceId = InterfaceId([
    0x30, 0x3a, 0x73, 0x0c, 0x1c, 0x2a, 0xce, 0x11, 0xad, 0xe5, 0, 0xaa, 0, 0x44, 0x77, 0x3d,
]);
