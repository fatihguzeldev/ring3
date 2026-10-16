pub(crate) const MOUSE: u32 = 0x5900_0001;
pub(crate) const KEYBOARD: u32 = 0x5900_0002;
pub(crate) const NO_LEGACY: u32 = 0x30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawRegistration32 {
    pub page: u16,
    pub usage: u16,
    pub flags: u32,
    pub window: u32,
}
#[derive(Clone, Copy)]
pub(crate) struct RawPacket32 {
    pub id: u32,
    pub window: u32,
    pub thread: u32,
    pub bytes: [u8; 40],
    pub length: usize,
    pub queued: bool,
}
