use crate::windows::com::InterfaceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InterfaceKind {
    Device = 1,
    Device8 = 2,
    Buffer = 3,
    Buffer8 = 4,
    Notify = 5,
    Listener = 6,
    Buffer3D = 7,
    Capture = 8,
    CaptureBuffer = 9,
    CaptureBuffer8 = 10,
    FullDuplex = 11,
}

const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> InterfaceId {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    InterfaceId([
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ])
}

impl InterfaceKind {
    pub const ALL: [Self; 11] = [
        Self::Device,
        Self::Device8,
        Self::Buffer,
        Self::Buffer8,
        Self::Notify,
        Self::Listener,
        Self::Buffer3D,
        Self::Capture,
        Self::CaptureBuffer,
        Self::CaptureBuffer8,
        Self::FullDuplex,
    ];
    pub fn from_tag(tag: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as u8 == tag)
    }
    pub const fn iid(self) -> InterfaceId {
        match self {
            Self::Device => guid(
                0x279afa83,
                0x4981,
                0x11ce,
                [0xa5, 0x21, 0, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
            ),
            Self::Device8 => guid(
                0xc50a7e93,
                0xf395,
                0x4834,
                [0x9e, 0xf6, 0x7f, 0xa9, 0x9d, 0xe5, 9, 0x66],
            ),
            Self::Buffer => guid(
                0x279afa85,
                0x4981,
                0x11ce,
                [0xa5, 0x21, 0, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
            ),
            Self::Buffer8 => guid(
                0x6825a449,
                0x7524,
                0x4d82,
                [0x92, 0x0f, 0x50, 0xe3, 0x6a, 0xb3, 0xab, 0x1e],
            ),
            Self::Notify => guid(
                0xb0210783,
                0x89cd,
                0x11d0,
                [0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16],
            ),
            Self::Listener => guid(
                0x279afa84,
                0x4981,
                0x11ce,
                [0xa5, 0x21, 0, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
            ),
            Self::Buffer3D => guid(
                0x279afa86,
                0x4981,
                0x11ce,
                [0xa5, 0x21, 0, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
            ),
            Self::Capture => guid(
                0xb0210781,
                0x89cd,
                0x11d0,
                [0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16],
            ),
            Self::CaptureBuffer => guid(
                0xb0210782,
                0x89cd,
                0x11d0,
                [0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16],
            ),
            Self::CaptureBuffer8 => guid(
                0x00990df4,
                0x0dbb,
                0x4872,
                [0x83, 0x3e, 0x6d, 0x30, 0x3e, 0x80, 0xae, 0xb6],
            ),
            Self::FullDuplex => guid(
                0xedcb4c7a,
                0xdaab,
                0x4216,
                [0xa4, 0x2e, 0x6c, 0x50, 0x59, 0x6d, 0xdc, 0x1d],
            ),
        }
    }
}
