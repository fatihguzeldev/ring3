use super::ResourceError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Format {
    R8G8B8 = 20,
    A8R8G8B8 = 21,
    X8R8G8B8 = 22,
    R5G6B5 = 23,
    X1R5G5B5 = 24,
    A1R5G5B5 = 25,
    A4R4G4B4 = 26,
    R3G3B2 = 27,
    A8 = 28,
    A8R3G3B2 = 29,
    X4R4G4B4 = 30,
    A2B10G10R10 = 31,
    G16R16 = 34,
    A8P8 = 40,
    P8 = 41,
    L8 = 50,
    A8L8 = 51,
    A4L4 = 52,
    V8U8 = 60,
    L6V5U5 = 61,
    X8L8V8U8 = 62,
    Q8W8V8U8 = 63,
    V16U16 = 64,
    W11V11U10 = 65,
    A2W10V10U10 = 67,
    D16Lockable = 70,
    D32 = 71,
    D15S1 = 73,
    D24S8 = 75,
    D24X8 = 77,
    D24X4S4 = 79,
    D16 = 80,
    VertexData = 100,
    Index16 = 101,
    Index32 = 102,
    Uyvy = u32::from_le_bytes(*b"UYVY"),
    Yuy2 = u32::from_le_bytes(*b"YUY2"),
    Dxt1 = u32::from_le_bytes(*b"DXT1"),
    Dxt2 = u32::from_le_bytes(*b"DXT2"),
    Dxt3 = u32::from_le_bytes(*b"DXT3"),
    Dxt4 = u32::from_le_bytes(*b"DXT4"),
    Dxt5 = u32::from_le_bytes(*b"DXT5"),
}

impl Format {
    pub const fn is_depth(self) -> bool {
        matches!(
            self,
            Self::D16Lockable
                | Self::D32
                | Self::D15S1
                | Self::D24S8
                | Self::D24X8
                | Self::D24X4S4
                | Self::D16
        )
    }

    pub const fn is_buffer(self) -> bool {
        matches!(self, Self::VertexData | Self::Index16 | Self::Index32)
    }

    pub const fn block(self) -> (u32, u32, u32) {
        match self {
            Self::Dxt1 => (4, 4, 8),
            Self::Dxt2 | Self::Dxt3 | Self::Dxt4 | Self::Dxt5 => (4, 4, 16),
            Self::Uyvy | Self::Yuy2 => (2, 1, 4),
            Self::R8G8B8 => (1, 1, 3),
            Self::R3G3B2 | Self::A8 | Self::P8 | Self::L8 | Self::A4L4 | Self::VertexData => {
                (1, 1, 1)
            }
            Self::R5G6B5
            | Self::X1R5G5B5
            | Self::A1R5G5B5
            | Self::A4R4G4B4
            | Self::A8R3G3B2
            | Self::X4R4G4B4
            | Self::A8P8
            | Self::A8L8
            | Self::V8U8
            | Self::L6V5U5
            | Self::D16Lockable
            | Self::D15S1
            | Self::D16
            | Self::Index16 => (1, 1, 2),
            _ => (1, 1, 4),
        }
    }
}

impl TryFrom<u32> for Format {
    type Error = ResourceError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            20 => Self::R8G8B8,
            21 => Self::A8R8G8B8,
            22 => Self::X8R8G8B8,
            23 => Self::R5G6B5,
            24 => Self::X1R5G5B5,
            25 => Self::A1R5G5B5,
            26 => Self::A4R4G4B4,
            27 => Self::R3G3B2,
            28 => Self::A8,
            29 => Self::A8R3G3B2,
            30 => Self::X4R4G4B4,
            31 => Self::A2B10G10R10,
            34 => Self::G16R16,
            40 => Self::A8P8,
            41 => Self::P8,
            50 => Self::L8,
            51 => Self::A8L8,
            52 => Self::A4L4,
            60 => Self::V8U8,
            61 => Self::L6V5U5,
            62 => Self::X8L8V8U8,
            63 => Self::Q8W8V8U8,
            64 => Self::V16U16,
            65 => Self::W11V11U10,
            67 => Self::A2W10V10U10,
            70 => Self::D16Lockable,
            71 => Self::D32,
            73 => Self::D15S1,
            75 => Self::D24S8,
            77 => Self::D24X8,
            79 => Self::D24X4S4,
            80 => Self::D16,
            100 => Self::VertexData,
            101 => Self::Index16,
            102 => Self::Index32,
            x if x == Self::Uyvy as u32 => Self::Uyvy,
            x if x == Self::Yuy2 as u32 => Self::Yuy2,
            x if x == Self::Dxt1 as u32 => Self::Dxt1,
            x if x == Self::Dxt2 as u32 => Self::Dxt2,
            x if x == Self::Dxt3 as u32 => Self::Dxt3,
            x if x == Self::Dxt4 as u32 => Self::Dxt4,
            x if x == Self::Dxt5 as u32 => Self::Dxt5,
            _ => return Err(ResourceError::NotAvailable),
        })
    }
}
