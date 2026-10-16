use crate::windows::com::InterfaceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureEffectKind {
    Aec,
    NoiseSuppress,
}
const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ]
}
pub const fn class_id(kind: CaptureEffectKind) -> [u8; 16] {
    match kind {
        CaptureEffectKind::Aec => guid(
            0xbf963d80,
            0xc559,
            0x11d0,
            [0x8a, 0x2b, 0, 0xa0, 0xc9, 0x25, 0x5a, 0xc1],
        ),
        CaptureEffectKind::NoiseSuppress => guid(
            0xe07f903f,
            0x62fd,
            0x4e60,
            [0x8c, 0xdd, 0xde, 0xa7, 0x23, 0x66, 0x65, 0xb5],
        ),
    }
}
pub const fn instance_id(kind: CaptureEffectKind, microsoft: bool) -> [u8; 16] {
    match (kind, microsoft) {
        (CaptureEffectKind::Aec, true) => guid(
            0xcdebb919,
            0x379a,
            0x488a,
            [0x87, 0x65, 0xf5, 0x3c, 0xfd, 0x36, 0xde, 0x40],
        ),
        (CaptureEffectKind::Aec, false) => guid(
            0x1c22c56d,
            0x9879,
            0x4f5b,
            [0xa3, 0x89, 0x27, 0x99, 0x6d, 0xdc, 0x28, 0x10],
        ),
        (CaptureEffectKind::NoiseSuppress, true) => guid(
            0x11c5c73b,
            0x66e9,
            0x4ba1,
            [0xa0, 0xba, 0xe8, 0x14, 0xc6, 0xee, 0xd9, 0x2d],
        ),
        (CaptureEffectKind::NoiseSuppress, false) => guid(
            0x5ab0882e,
            0x7274,
            0x4516,
            [0x87, 0x7d, 0x4e, 0xee, 0x99, 0xba, 0x4f, 0xd0],
        ),
    }
}
pub const fn interface_id(kind: CaptureEffectKind) -> InterfaceId {
    InterfaceId(match kind {
        CaptureEffectKind::Aec => guid(
            0xad74143d,
            0x903d,
            0x4ab7,
            [0x80, 0x66, 0x28, 0xd3, 0x63, 3, 0x6d, 0x65],
        ),
        CaptureEffectKind::NoiseSuppress => guid(
            0xed311e41,
            0xfbae,
            0x4175,
            [0x96, 0x25, 0xcd, 8, 0x54, 0xf6, 0x93, 0xca],
        ),
    })
}
