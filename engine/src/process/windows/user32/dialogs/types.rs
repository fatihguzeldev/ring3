#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum User32DialogIcon32<'a> {
    Standard(u32),
    Resource {
        instance: u32,
        id: Option<u16>,
        name: &'a [u16],
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct User32DialogRequest32<'a> {
    pub epoch: u64,
    pub id: u64,
    pub thread: u32,
    pub owner: u32,
    pub style: u32,
    pub language: u16,
    pub help_context: u32,
    pub caption: &'a [u16],
    pub text: &'a [u16],
    pub icon: User32DialogIcon32<'a>,
}
pub(in crate::process::windows::user32) struct Dialog32 {
    pub id: u64,
    pub thread: u32,
    pub owner: u32,
    pub style: u32,
    pub language: u16,
    pub help_context: u32,
    pub help_callback: u32,
    pub instance: u32,
    pub caption: Vec<u16>,
    pub text: Vec<u16>,
    pub icon_id: Option<u16>,
    pub icon_name: Vec<u16>,
    pub result: Option<(u32, u32)>,
    pub help: bool,
}
impl Dialog32 {
    pub(super) fn request(&self, epoch: u64) -> User32DialogRequest32<'_> {
        User32DialogRequest32 {
            epoch,
            id: self.id,
            thread: self.thread,
            owner: self.owner,
            style: self.style,
            language: self.language,
            help_context: self.help_context,
            caption: &self.caption,
            text: &self.text,
            icon: if self.style & 0x80 != 0 {
                User32DialogIcon32::Resource {
                    instance: self.instance,
                    id: self.icon_id,
                    name: &self.icon_name,
                }
            } else {
                User32DialogIcon32::Standard(self.style & 0xf0)
            },
        }
    }
    pub(super) fn buttons(&self) -> &'static [u32] {
        buttons(self.style)
    }
}
pub(super) fn buttons(style: u32) -> &'static [u32] {
    match style & 0xf {
        0 => &[1],
        1 => &[1, 2],
        2 => &[3, 4, 5],
        3 => &[6, 7, 2],
        4 => &[6, 7],
        5 => &[4, 2],
        6 => &[2, 10, 11],
        _ => &[],
    }
}
