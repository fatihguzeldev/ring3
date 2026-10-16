use crate::{
    input::dinput::{DirectInputResult32, format::ObjectSelector32},
    windows::com::InterfaceId,
};
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceKind32 {
    Input1A = 1,
    Input1W = 2,
    Input2A = 3,
    Input2W = 4,
    Input7A = 5,
    Input7W = 6,
    Input8A = 7,
    Input8W = 8,
    Device1A = 9,
    Device1W = 10,
    Device2A = 11,
    Device2W = 12,
    Device7A = 13,
    Device7W = 14,
    Device8A = 15,
    Device8W = 16,
    EffectLegacy = 17,
    Effect8 = 18,
}
impl TryFrom<u32> for InterfaceKind32 {
    type Error = DirectInputResult32;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|kind| *kind as u32 == value)
            .ok_or(DirectInputResult32::INVALID_PARAM)
    }
}
impl InterfaceKind32 {
    pub const ALL: [Self; 18] = [
        Self::Input1A,
        Self::Input1W,
        Self::Input2A,
        Self::Input2W,
        Self::Input7A,
        Self::Input7W,
        Self::Input8A,
        Self::Input8W,
        Self::Device1A,
        Self::Device1W,
        Self::Device2A,
        Self::Device2W,
        Self::Device7A,
        Self::Device7W,
        Self::Device8A,
        Self::Device8W,
        Self::EffectLegacy,
        Self::Effect8,
    ];
    pub fn iid(self) -> InterfaceId {
        InterfaceId(match self {
            Self::Input1A => [
                0x60, 0x13, 0x52, 0x89, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Input1W => [
                0x61, 0x13, 0x52, 0x89, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Input2A => [
                0x62, 0xe6, 0x44, 0x59, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Input2W => [
                0x63, 0xe6, 0x44, 0x59, 0x8a, 0xaa, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Input7A => [
                0x84, 0xb6, 0x4c, 0x9a, 0x6d, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0x00, 0xc0, 0x4f, 0x68,
                0x44, 0xae,
            ],
            Self::Input7W => [
                0x85, 0xb6, 0x4c, 0x9a, 0x6d, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0x00, 0xc0, 0x4f, 0x68,
                0x44, 0xae,
            ],
            Self::Input8A => [
                0x30, 0x80, 0x79, 0xbf, 0x3a, 0x48, 0xa2, 0x4d, 0xaa, 0x99, 0x5d, 0x64, 0xed, 0x36,
                0x97, 0x00,
            ],
            Self::Input8W => [
                0x31, 0x80, 0x79, 0xbf, 0x3a, 0x48, 0xa2, 0x4d, 0xaa, 0x99, 0x5d, 0x64, 0xed, 0x36,
                0x97, 0x00,
            ],
            Self::Device1A => [
                0x80, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Device1W => [
                0x81, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Device2A => [
                0x82, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Device2W => [
                0x83, 0xe6, 0x44, 0x59, 0x2e, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0x00, 0x00,
            ],
            Self::Device7A => [
                0xbc, 0xc6, 0xd7, 0x57, 0x56, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0x00, 0xc0, 0x4f, 0x68,
                0x44, 0xae,
            ],
            Self::Device7W => [
                0xbd, 0xc6, 0xd7, 0x57, 0x56, 0x23, 0xd3, 0x11, 0x8e, 0x9d, 0x00, 0xc0, 0x4f, 0x68,
                0x44, 0xae,
            ],
            Self::Device8A => [
                0x80, 0x10, 0xd4, 0x54, 0x15, 0xdc, 0x33, 0x48, 0xa4, 0x1b, 0x74, 0x8f, 0x73, 0xa3,
                0x81, 0x79,
            ],
            Self::EffectLegacy | Self::Effect8 => [
                0xc0, 0xf7, 0xe1, 0xe7, 0xd2, 0x88, 0xd0, 0x11, 0x9a, 0xd0, 0, 0xa0, 0xc9, 0xa0,
                0x6e, 0x35,
            ],
            Self::Device8W => [
                0x81, 0x10, 0xd4, 0x54, 0x15, 0xdc, 0x33, 0x48, 0xa4, 0x1b, 0x74, 0x8f, 0x73, 0xa3,
                0x81, 0x79,
            ],
        })
    }
    pub fn from_iid(iid: InterfaceId) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.iid() == iid)
    }
    pub fn wide(self) -> bool {
        (self as u8).is_multiple_of(2)
    }
    pub fn device(self) -> bool {
        matches!(self as u8, 9..=16)
    }
    pub fn effect(self) -> bool {
        matches!(self, Self::EffectLegacy | Self::Effect8)
    }
    pub fn generation(self) -> u8 {
        match self as u8 {
            1 | 2 | 9 | 10 => 1,
            3 | 4 | 11 | 12 => 2,
            5 | 6 | 13 | 14 => 7,
            17 => 7,
            _ => 8,
        }
    }
    pub fn device_interface(self) -> Self {
        Self::try_from(u32::from(self as u8) + 8).unwrap()
    }
    pub fn words(self) -> &'static [u8] {
        if self.effect() {
            return &[3, 1, 1, 4, 2, 3, 3, 3, 1, 2, 1, 1, 2];
        }
        if !self.device() {
            match self.generation() {
                1 => &[3, 1, 1, 4, 5, 2, 3, 3],
                2 => &[3, 1, 1, 4, 5, 2, 3, 3, 4],
                7 => &[3, 1, 1, 4, 5, 2, 3, 3, 4, 5],
                _ => &[3, 1, 1, 4, 5, 2, 3, 3, 4, 6, 5],
            }
        } else {
            match self.generation() {
                1 => &[3, 1, 1, 2, 4, 3, 3, 1, 1, 3, 5, 2, 2, 3, 4, 2, 3, 4],
                2 => &[
                    3, 1, 1, 2, 4, 3, 3, 1, 1, 3, 5, 2, 2, 3, 4, 2, 3, 4, 5, 4, 3, 2, 2, 4, 2, 1, 5,
                ],
                7 => &[
                    3, 1, 1, 2, 4, 3, 3, 1, 1, 3, 5, 2, 2, 3, 4, 2, 3, 4, 5, 4, 3, 2, 2, 4, 2, 1,
                    5, 5, 5,
                ],
                _ => &[
                    3, 1, 1, 2, 4, 3, 3, 1, 1, 3, 5, 2, 2, 3, 4, 2, 3, 4, 5, 4, 3, 2, 2, 4, 2, 1,
                    5, 5, 5, 4, 4, 2,
                ],
            }
        }
    }
    pub fn compatible(self, other: Self) -> bool {
        if self.effect() || other.effect() {
            return self == other;
        }
        self.device() == other.device() && (self.generation() == 8) == (other.generation() == 8)
    }
}
pub const SYSTEM_MOUSE: [u8; 16] = [
    0x60, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];
pub const SYSTEM_KEYBOARD: [u8; 16] = [
    0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];
pub fn object_selector(guid: [u8; 16]) -> ObjectSelector32 {
    match guid {
        [
            0xe0,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(0),
        [
            0xe1,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(1),
        [
            0xe2,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(2),
        [
            0xf4,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(3),
        [
            0xf5,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(4),
        [
            0xe3,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Axis(5),
        [
            0xe4,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Slider,
        [
            0xf0,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Button,
        [
            0x20,
            0x82,
            0x72,
            0x55,
            0x3c,
            0xd3,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Key,
        [
            0xf2,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Pov,
        [
            0xf3,
            0x02,
            0x6d,
            0xa3,
            0xf3,
            0xc9,
            0xcf,
            0x11,
            0xbf,
            0xc7,
            0x44,
            0x45,
            0x53,
            0x54,
            0x00,
            0x00,
        ] => ObjectSelector32::Unknown,
        _ => ObjectSelector32::Unknown,
    }
}
