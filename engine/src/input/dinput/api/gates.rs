use super::InterfaceKind32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactoryGate {
    Create8,
    CreateA,
    CreateW,
    CreateEx,
}
impl FactoryGate {
    pub const ALL: [Self; 4] = [Self::Create8, Self::CreateA, Self::CreateW, Self::CreateEx];
    pub fn id(self) -> u32 {
        0x0008_0000
            | match self {
                Self::Create8 => 1,
                Self::CreateA => 2,
                Self::CreateW => 3,
                Self::CreateEx => 4,
            }
    }
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|gate| gate.id() == id)
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::CreateA | Self::CreateW => 4,
            _ => 5,
        }
    }
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Create8 => "DirectInput8Create",
            Self::CreateA => "DirectInputCreateA",
            Self::CreateW => "DirectInputCreateW",
            Self::CreateEx => "DirectInputCreateEx",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodInfo32 {
    pub stack_words: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodGate {
    pub interface: InterfaceKind32,
    pub slot: u8,
}
impl MethodGate {
    pub fn id(self) -> u32 {
        0x0009_0000 | (self.interface as u32) << 8 | u32::from(self.slot)
    }
    pub fn from_id(id: u32) -> Option<Self> {
        if id & 0xffff_0000 != 0x0009_0000 {
            return None;
        }
        let interface = InterfaceKind32::try_from((id >> 8) & 255).ok()?;
        let slot = (id & 255) as u8;
        (usize::from(slot) < interface.words().len()).then_some(Self { interface, slot })
    }
    pub fn stack_words(self) -> u8 {
        self.interface.words()[usize::from(self.slot)]
    }
    pub fn method(self) -> MethodInfo32 {
        MethodInfo32 {
            stack_words: self.stack_words(),
        }
    }
}
