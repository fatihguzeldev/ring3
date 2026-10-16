#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamMethod32(pub u8);
impl StreamMethod32 {
    pub const ALL: [Self; 14] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
        Self(8),
        Self(9),
        Self(10),
        Self(11),
        Self(12),
        Self(13),
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        (0x000d_1100..0x000d_110e)
            .contains(&id)
            .then(|| Self((id - 0x000d_1100) as u8))
    }
    pub fn id(self) -> u32 {
        0x000d_1100 + u32::from(self.0)
    }
    pub fn stack_words(self) -> u32 {
        [3, 1, 1, 4, 4, 5, 3, 6, 2, 1, 6, 6, 3, 2][usize::from(self.0)]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpcChannelMethod32(pub u8);
impl RpcChannelMethod32 {
    pub const ALL: [Self; 8] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        (0x000d_1200..0x000d_1208)
            .contains(&id)
            .then(|| Self((id - 0x000d_1200) as u8))
    }
    pub fn id(self) -> u32 {
        0x000d_1200 + u32::from(self.0)
    }
    pub fn stack_words(self) -> u32 {
        [3, 1, 1, 3, 3, 2, 3, 1][usize::from(self.0)]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x86_method_tables_preserve_large_integer_by_value_stack_words() {
        assert_eq!(
            StreamMethod32::ALL.map(StreamMethod32::stack_words),
            [3, 1, 1, 4, 4, 5, 3, 6, 2, 1, 6, 6, 3, 2]
        );
        assert_eq!(
            RpcChannelMethod32::ALL.map(RpcChannelMethod32::stack_words),
            [3, 1, 1, 3, 3, 2, 3, 1]
        );
        assert_eq!(StreamMethod32::from_id(0x000d110e), None);
        assert_eq!(RpcChannelMethod32::from_id(0x000d1208), None);
    }
}
