/// stack operations use the configured flat 32-bit stack-address profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StackFrameKind {
    PushAll32 = 0,
    PushAll16 = 1,
    PopAll32 = 2,
    PopAll16 = 3,
    Enter32 = 4,
    Enter16 = 5,
}

impl StackFrameKind {
    pub const fn width(self) -> u32 {
        match self {
            Self::PushAll16 | Self::PopAll16 | Self::Enter16 => 2,
            _ => 4,
        }
    }

    pub const fn control(self, allocation: u16, nesting: u8) -> u32 {
        self as u32 | ((allocation as u32) << 8) | (((nesting & 31) as u32) << 24)
    }

    pub const fn decode(control: u32) -> Option<(Self, u16, u8)> {
        if control & !0x1fff_ff07 != 0 {
            return None;
        }
        let kind = match control & 7 {
            0 => Self::PushAll32,
            1 => Self::PushAll16,
            2 => Self::PopAll32,
            3 => Self::PopAll16,
            4 => Self::Enter32,
            5 => Self::Enter16,
            _ => return None,
        };
        let allocation = (control >> 8) as u16;
        let nesting = (control >> 24) as u8;
        if !matches!(kind, Self::Enter32 | Self::Enter16) && (allocation != 0 || nesting != 0) {
            return None;
        }
        Some((kind, allocation, nesting))
    }
}
