#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerKind32 {
    Joystick,
    Gamepad,
    Driving,
    Flight,
    FirstPerson,
}
impl TryFrom<u32> for ControllerKind32 {
    type Error = DirectInputResult32;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Joystick),
            2 => Ok(Self::Gamepad),
            3 => Ok(Self::Driving),
            4 => Ok(Self::Flight),
            5 => Ok(Self::FirstPerson),
            _ => Err(DirectInputResult32::INVALID_PARAM),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerDescriptor32 {
    pub instance: [u8; 16],
    pub product: [u8; 16],
    pub name: Vec<u16>,
    pub axes: u8,
    pub povs: u8,
    pub buttons: u16,
    pub kind: ControllerKind32,
}
impl ControllerDescriptor32 {
    pub fn validate(&self) -> Result<(), DirectInputResult32> {
        if self.instance == [0; 16]
            || self.product == [0; 16]
            || self.name.is_empty()
            || self.name.len() > 259
            || self.name.contains(&0)
            || self.axes > 8
            || self.povs > 4
            || self.buttons > 128
            || self.axes == 0 && self.buttons == 0 && self.povs == 0
        {
            Err(DirectInputResult32::INVALID_PARAM)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind32 {
    Keyboard,
    Mouse,
    Controller(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectInputResult32(pub u32);
impl DirectInputResult32 {
    pub const OK: Self = Self(0);
    pub const NO_EFFECT: Self = Self(1);
    pub const BUFFER_OVERFLOW: Self = Self(1);
    pub const INVALID_PARAM: Self = Self(0x8007_0057);
    pub const POINTER: Self = Self(0x8000_4003);
    pub const NO_INTERFACE: Self = Self(0x8000_4002);
    pub const NOT_INITIALIZED: Self = Self(0x8007_0015);
    pub const NOT_ACQUIRED: Self = Self(0x8007_000c);
    pub const INPUT_LOST: Self = Self(0x8007_001e);
    pub const ACQUIRED: Self = Self(0x8007_00aa);
    pub const OTHER_APP: Self = Self(0x8007_0005);
    pub const OBJECT_NOT_FOUND: Self = Self(0x8007_0002);
    pub const NOT_BUFFERED: Self = Self(0x8004_0207);
    pub const UNSUPPORTED: Self = Self(0x8000_4001);
    pub const OUT_OF_MEMORY: Self = Self(0x8007_000e);
    pub const DEVICE_NOT_REGISTERED: Self = Self(0x8004_0154);
    pub const NO_AGGREGATION: Self = Self(0x8004_0110);
    pub const HANDLE: Self = Self(0x8007_0006);
}
