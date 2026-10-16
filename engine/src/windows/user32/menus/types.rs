use super::super::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MenuHandle32(pub(super) u32);

impl MenuHandle32 {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for MenuHandle32 {
    type Error = E;
    fn try_from(value: u32) -> Result<Self, E> {
        (value != 0).then_some(Self(value)).ok_or(E::Win32(1401))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MenuLimits32 {
    pub menus: usize,
    pub items: usize,
    pub text_units: usize,
    pub depth: usize,
}

impl Default for MenuLimits32 {
    fn default() -> Self {
        Self {
            menus: 256,
            items: 4096,
            text_units: 512 * 1024,
            depth: 32,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MenuItem32 {
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub text: Vec<u16>,
    pub submenu: Option<MenuHandle32>,
    pub data: u32,
    pub bitmap: u32,
    pub checked_bitmap: u32,
    pub unchecked_bitmap: u32,
}

impl MenuItem32 {
    pub(crate) fn command(id: u32, text: Vec<u16>) -> Self {
        Self {
            id,
            kind: 0,
            state: 0,
            text,
            submenu: None,
            data: 0,
            bitmap: 0,
            checked_bitmap: 0,
            unchecked_bitmap: 0,
        }
    }
}

pub(super) struct Menu32 {
    pub handle: MenuHandle32,
    pub popup: bool,
    pub help: u32,
    pub items: Vec<MenuItem32>,
}
