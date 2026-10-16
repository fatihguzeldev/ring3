use crate::windows::com::InterfaceId;
pub(crate) const IID_IRPCCHANNELBUFFER: InterfaceId = InterfaceId([
    0x60, 0x6b, 0xf5, 0xd5, 0x3b, 0x59, 0x1a, 0x10, 0xb5, 0x69, 8, 0, 0x2b, 0x2d, 0xbf, 0x7a,
]);
const fn rpc_iid(first: u32) -> InterfaceId {
    let mut bytes = IID_IRPCCHANNELBUFFER.0;
    let first = first.to_le_bytes();
    bytes[0] = first[0];
    bytes[1] = first[1];
    bytes[2] = first[2];
    bytes[3] = first[3];
    InterfaceId(bytes)
}
pub(crate) const IID_IPSFACTORYBUFFER: InterfaceId = rpc_iid(0xd5f569d0);
