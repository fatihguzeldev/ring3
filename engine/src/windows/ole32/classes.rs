use crate::windows::com::InterfaceId;
pub const IID_ICLASSFACTORY: InterfaceId =
    InterfaceId([1, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownClass32 {
    DirectSound,
    DirectSound8,
    DirectSoundCapture,
    DirectSoundCapture8,
    DirectSoundFullDuplex,
    DirectInput,
    DirectInput8,
}
impl KnownClass32 {
    pub const ALL: [Self; 7] = [
        Self::DirectSound,
        Self::DirectSound8,
        Self::DirectSoundCapture,
        Self::DirectSoundCapture8,
        Self::DirectSoundFullDuplex,
        Self::DirectInput,
        Self::DirectInput8,
    ];
    pub fn from_id(id: InterfaceId) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.id() == id)
    }
    pub fn id(self) -> InterfaceId {
        let (a, b, c, tail): (u32, u16, u16, [u8; 8]) = match self {
            Self::DirectSound => (
                0x47d4d946,
                0x62e8,
                0x11cf,
                [0x93, 0xbc, 0x44, 0x45, 0x53, 0x54, 0, 0],
            ),
            Self::DirectSound8 => (
                0x3901cc3f,
                0x84b5,
                0x4fa4,
                [0xba, 0x35, 0xaa, 0x81, 0x72, 0xb8, 0xa0, 0x9b],
            ),
            Self::DirectSoundCapture => (
                0xb0210780,
                0x89cd,
                0x11d0,
                [0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16],
            ),
            Self::DirectSoundCapture8 => (
                0xe4bcac13,
                0x7f99,
                0x4908,
                [0x9a, 0x8e, 0x74, 0xe3, 0xbf, 0x24, 0xb6, 0xe1],
            ),
            Self::DirectSoundFullDuplex => (
                0xfea4300c,
                0x7959,
                0x4147,
                [0xb2, 0x6a, 0x23, 0x77, 0xb9, 0xe7, 0xa9, 0x1d],
            ),
            Self::DirectInput => (
                0x25e609e0,
                0xb259,
                0x11cf,
                [0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0],
            ),
            Self::DirectInput8 => (
                0x25e609e4,
                0xb259,
                0x11cf,
                [0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0],
            ),
        };
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&a.to_le_bytes());
        bytes[4..6].copy_from_slice(&b.to_le_bytes());
        bytes[6..8].copy_from_slice(&c.to_le_bytes());
        bytes[8..].copy_from_slice(&tail);
        InterfaceId(bytes)
    }
}
