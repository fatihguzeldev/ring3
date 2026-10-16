#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RpcMessage32 {
    pub reserved: u32,
    pub representation: u32,
    pub buffer: u32,
    pub length: u32,
    pub method: u32,
    pub reserved2: [u32; 5],
    pub flags: u32,
}
impl RpcMessage32 {
    pub(crate) const SIZE: usize = 44;
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        Some(Self {
            reserved: word(0),
            representation: word(4),
            buffer: word(8),
            length: word(12),
            method: word(16),
            reserved2: core::array::from_fn(|index| word(20 + index * 4)),
            flags: word(40),
        })
    }
    pub(crate) fn encode(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (slot, value) in [
            self.reserved,
            self.representation,
            self.buffer,
            self.length,
            self.method,
            self.reserved2[0],
            self.reserved2[1],
            self.reserved2[2],
            self.reserved2[3],
            self.reserved2[4],
            self.flags,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}
