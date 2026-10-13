use super::super::VectorWidth;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GatherGeometry {
    DwordDword128 = 1,
    DwordDword256,
    QwordDword128,
    QwordDword256,
    DwordQword128,
    DwordQword256,
    QwordQword128,
    QwordQword256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GatherScale {
    One,
    Two,
    Four,
    Eight,
}

impl GatherGeometry {
    pub(crate) const fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::DwordDword128,
            2 => Self::DwordDword256,
            3 => Self::QwordDword128,
            4 => Self::QwordDword256,
            5 => Self::DwordQword128,
            6 => Self::DwordQword256,
            7 => Self::QwordQword128,
            8 => Self::QwordQword256,
            _ => return None,
        })
    }

    pub(crate) const fn id(self) -> u8 {
        self as u8
    }

    pub(crate) const fn data_bytes(self) -> usize {
        match self {
            Self::DwordDword128
            | Self::DwordDword256
            | Self::DwordQword128
            | Self::DwordQword256 => 4,
            _ => 8,
        }
    }

    pub(crate) const fn index_bytes(self) -> usize {
        match self {
            Self::DwordDword128
            | Self::DwordDword256
            | Self::QwordDword128
            | Self::QwordDword256 => 4,
            _ => 8,
        }
    }

    pub(crate) const fn vector_length(self) -> VectorWidth {
        match self {
            Self::DwordDword128
            | Self::QwordDword128
            | Self::DwordQword128
            | Self::QwordQword128 => VectorWidth::Xmm,
            _ => VectorWidth::Ymm,
        }
    }

    pub(crate) const fn lanes(self) -> usize {
        match self {
            Self::DwordDword128
            | Self::QwordDword256
            | Self::DwordQword256
            | Self::QwordQword256 => 4,
            Self::DwordDword256 => 8,
            _ => 2,
        }
    }

    pub(crate) const fn active_bytes(self) -> usize {
        self.lanes() * self.data_bytes()
    }

    pub(crate) const fn index_register_bytes(self) -> usize {
        match self {
            Self::DwordDword256 | Self::DwordQword256 | Self::QwordQword256 => 32,
            _ => 16,
        }
    }

    pub(crate) const fn early_zero_unused_destination(self) -> bool {
        self.data_bytes() != self.index_bytes()
    }
}

impl GatherScale {
    pub(crate) const fn from_power(power: u8) -> Option<Self> {
        Some(match power {
            0 => Self::One,
            1 => Self::Two,
            2 => Self::Four,
            3 => Self::Eight,
            _ => return None,
        })
    }

    pub(crate) const fn power(self) -> u8 {
        self as u8
    }

    pub(crate) const fn multiplier(self) -> u32 {
        1 << self.power()
    }
}

#[cfg(test)]
#[path = "geometry/native.rs"]
mod tests;
