use super::super::{DeviceKind32, DirectInputResult32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceObject32 {
    Key(u16),
    Button(u16),
    Axis { index: u8, aspect: u8 },
    Pov(u8),
}
impl DeviceObject32 {
    pub fn width(self) -> u32 {
        match self {
            Self::Key(_) | Self::Button(_) => 1,
            _ => 4,
        }
    }
    pub fn instance(self) -> u16 {
        match self {
            Self::Key(i) | Self::Button(i) => i,
            Self::Axis { index, .. } => u16::from(index),
            Self::Pov(i) => u16::from(i),
        }
    }
    pub fn object_type(self, relative: bool) -> u32 {
        let kind = match self {
            Self::Key(_) | Self::Button(_) => 4,
            Self::Axis { .. } => {
                if relative {
                    1
                } else {
                    2
                }
            }
            Self::Pov(_) => 16,
        };
        kind | u32::from(self.instance()) << 8
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectType32 {
    Any,
    Axis,
    Button,
    Pov,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectSelector32 {
    Any,
    Key,
    Button,
    Pov,
    Axis(u8),
    Slider,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDescription32 {
    pub kind: DeviceKind32,
    pub axes: u8,
    pub povs: u8,
    pub buttons: u16,
}
impl DeviceDescription32 {
    pub fn keyboard() -> Self {
        Self {
            kind: DeviceKind32::Keyboard,
            axes: 0,
            povs: 0,
            buttons: 256,
        }
    }
    pub fn mouse() -> Self {
        Self {
            kind: DeviceKind32::Mouse,
            axes: 3,
            povs: 0,
            buttons: 8,
        }
    }
    pub fn validate(self) -> Result<(), DirectInputResult32> {
        if self.axes > 8
            || self.povs > 4
            || self.buttons
                > if self.kind == DeviceKind32::Keyboard {
                    256
                } else {
                    128
                }
        {
            Err(DirectInputResult32::INVALID_PARAM)
        } else {
            Ok(())
        }
    }
    pub fn objects(self) -> impl Iterator<Item = DeviceObject32> {
        (0..self.axes)
            .map(|index| DeviceObject32::Axis { index, aspect: 0 })
            .chain((0..self.povs).map(DeviceObject32::Pov))
            .chain((0..self.buttons).map(move |i| {
                if self.kind == DeviceKind32::Keyboard {
                    DeviceObject32::Key(i)
                } else {
                    DeviceObject32::Button(i)
                }
            }))
    }
}

pub(super) fn matches(
    object: DeviceObject32,
    selector: ObjectSelector32,
    kind: ObjectType32,
    instance: Option<u16>,
    aspect: u8,
) -> bool {
    if instance.is_some_and(|i| object.instance() != i) {
        return false;
    }
    if !matches!(
        (kind, object),
        (ObjectType32::Any, _)
            | (ObjectType32::Axis, DeviceObject32::Axis { .. })
            | (
                ObjectType32::Button,
                DeviceObject32::Key(_) | DeviceObject32::Button(_)
            )
            | (ObjectType32::Pov, DeviceObject32::Pov(_))
    ) {
        return false;
    }
    match (selector, object) {
        (ObjectSelector32::Any, _) => true,
        (ObjectSelector32::Key, DeviceObject32::Key(_))
        | (ObjectSelector32::Button, DeviceObject32::Button(_))
        | (ObjectSelector32::Pov, DeviceObject32::Pov(_)) => true,
        (
            ObjectSelector32::Axis(wanted),
            DeviceObject32::Axis {
                index,
                aspect: object_aspect,
            },
        ) => index == wanted && aspect == object_aspect,
        (
            ObjectSelector32::Slider,
            DeviceObject32::Axis {
                index: 6 | 7,
                aspect: object_aspect,
            },
        ) => aspect == object_aspect,
        _ => false,
    }
}
