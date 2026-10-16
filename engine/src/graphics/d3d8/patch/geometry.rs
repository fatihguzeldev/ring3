use super::PatchError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Basis {
    Bezier = 0,
    BSpline = 1,
    Interpolate = 2,
}

impl TryFrom<u32> for Basis {
    type Error = PatchError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Bezier),
            1 => Ok(Self::BSpline),
            2 => Ok(Self::Interpolate),
            _ => Err(PatchError),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Degree {
    Linear = 1,
    Cubic = 3,
    Quintic = 5,
}

impl TryFrom<u32> for Degree {
    type Error = PatchError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Linear),
            3 => Ok(Self::Cubic),
            5 => Ok(Self::Quintic),
            _ => Err(PatchError),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectPatchInfo {
    pub start_width: u32,
    pub start_height: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub basis: Basis,
    pub degree: Degree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriPatchInfo {
    pub start: u32,
    pub vertices: u32,
    pub basis: Basis,
    pub degree: Degree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchInfo {
    Rectangle(RectPatchInfo),
    Triangle(TriPatchInfo),
}

impl PatchInfo {
    pub fn triangular(self) -> bool {
        matches!(self, Self::Triangle(_))
    }

    pub fn vertex_extent(self) -> Result<(u32, u32), PatchError> {
        match self {
            Self::Rectangle(info) => {
                let degree = info.degree as u32;
                let valid_dimensions = match info.basis {
                    Basis::Bezier => info.width == degree + 1 && info.height == degree + 1,
                    Basis::BSpline => info.width > degree && info.height > degree,
                    Basis::Interpolate => {
                        info.degree == Degree::Cubic && info.width > degree && info.height > degree
                    }
                };
                if !valid_dimensions
                    || info
                        .start_width
                        .checked_add(info.width)
                        .is_none_or(|end| end > info.stride)
                {
                    return Err(PatchError);
                }
                let first = info
                    .start_height
                    .checked_mul(info.stride)
                    .and_then(|value| value.checked_add(info.start_width))
                    .ok_or(PatchError)?;
                let count = (info.height - 1)
                    .checked_mul(info.stride)
                    .and_then(|value| value.checked_add(info.width))
                    .ok_or(PatchError)?;
                first.checked_add(count).ok_or(PatchError)?;
                Ok((first, count))
            }
            Self::Triangle(info) => {
                let count = match info.degree {
                    Degree::Linear => 3,
                    Degree::Cubic => 10,
                    Degree::Quintic => 21,
                };
                if info.basis != Basis::Bezier || info.vertices != count {
                    return Err(PatchError);
                }
                info.start.checked_add(count).ok_or(PatchError)?;
                Ok((info.start, count))
            }
        }
    }
}
