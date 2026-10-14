/// saved application context at a precise cpu stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInput32 {
    pub eip: u32,
    pub esp: u32,
    pub flags: u32,
    pub cs: u16,
}
