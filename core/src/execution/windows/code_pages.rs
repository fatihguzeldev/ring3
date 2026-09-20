const ANSI: u32 = 1252;
const OEM: u32 = 437;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Ansi,
    Oem,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x88 => Some(Self::Ansi),
            0x8c => Some(Self::Oem),
            _ => None,
        }
    }

    pub(super) fn identifier(self) -> u32 {
        match self {
            Self::Ansi => ANSI,
            Self::Oem => OEM,
        }
    }
}
