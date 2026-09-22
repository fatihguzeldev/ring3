use std::collections::BTreeMap;

use super::{
    DispatchError, GuestMemory, MemoryError,
    user_atoms::{self, UserAtoms},
};

pub(super) const DESKTOP: u32 = 1;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Desktop,
    Find,
    IsWindow,
    Active,
    DlgItem,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x10 => Some(Self::Desktop),
            0x26c => Some(Self::Find),
            0x270 => Some(Self::IsWindow),
            0x430 => Some(Self::Active),
            0x438 => Some(Self::DlgItem),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Desktop | Self::Active => 0,
            Self::IsWindow => 1,
            Self::Find | Self::DlgItem => 2,
        }
    }
}

#[derive(Default)]
pub(super) struct Window {
    pub(super) class: u32,
    pub(super) instance: u32,
    pub(super) procedure: u32,
    pub(super) style: u32,
    pub(super) exstyle: u32,
    pub(super) title: String,
    pub(super) rectangle: [i32; 4],
    pub(super) client: [i32; 4],
    pub(super) icons: [u32; 2],
    pub(super) parent: u32,
    pub(super) id: u32,
    pub(super) dialog_units: Option<[i16; 4]>,
}

pub(super) struct Desktop {
    top_levels: BTreeMap<u32, Window>,
    children: BTreeMap<u32, Window>,
    active: u32,
    next: u32,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            top_levels: BTreeMap::new(),
            children: BTreeMap::new(),
            active: 0,
            next: 0x7500_0004,
        }
    }
}

impl Desktop {
    pub(super) fn available(&self) -> Option<u32> {
        self.available_many(1)
    }
    pub(super) fn available_many(&self, count: usize) -> Option<u32> {
        let remaining = 4096_usize.checked_sub(self.top_levels.len() + self.children.len())?;
        let last = self
            .next
            .checked_add(u32::try_from(count.checked_sub(1)?).ok()?.checked_mul(4)?)?;
        (count <= remaining && last <= 0x75ff_fffc).then_some(self.next)
    }
    pub(super) fn insert(&mut self, handle: u32, window: Window) {
        self.top_levels.insert(handle, window);
        self.next += 4;
    }
    pub(super) fn insert_child(&mut self, handle: u32, window: Window) {
        self.children.insert(handle, window);
        self.next += 4;
    }
    pub(super) fn window(&self, handle: u32) -> Option<&Window> {
        self.top_levels
            .get(&handle)
            .or_else(|| self.children.get(&handle))
    }
    pub(super) fn window_mut(&mut self, handle: u32) -> Option<&mut Window> {
        if let Some(window) = self.top_levels.get_mut(&handle) {
            Some(window)
        } else {
            self.children.get_mut(&handle)
        }
    }
    pub(super) fn child(&self, parent: u32, id: u32) -> Option<u32> {
        self.children.iter().find_map(|(&handle, window)| {
            (window.parent == parent && window.id == id).then_some(handle)
        })
    }
    pub(super) fn remove(&mut self, handle: u32) {
        self.top_levels.remove(&handle);
        self.children.retain(|_, window| window.parent != handle);
        if self.active == handle {
            self.active = 0;
        }
    }
    pub(super) fn activate_created(&mut self, handle: u32) {
        if self
            .top_levels
            .get(&handle)
            .is_some_and(|window| window.style & 0x1000_0000 != 0)
        {
            self.active = handle;
        }
    }
    pub(super) fn has_class(&self, instance: u32, atom: u32) -> bool {
        self.top_levels
            .values()
            .any(|window| window.instance == instance && window.class == atom)
    }
    pub(super) fn dispatch(
        &self,
        call: Call,
        args: &[u32],
        atoms: &UserAtoms,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Desktop => Ok(DESKTOP),
            Call::Active => Ok(self.active),
            Call::IsWindow => Ok(u32::from(
                args[0] == DESKTOP
                    || self.top_levels.contains_key(&args[0])
                    || self.children.contains_key(&args[0]),
            )),
            Call::DlgItem => Ok(self.child(args[0], args[1]).unwrap_or(0)),
            Call::Find => {
                let class = match args[0] {
                    0 => None,
                    1..=0xbfff => return Err(DispatchError::Unsupported),
                    0xc000..=0xffff => Some(args[0]),
                    pointer => Some(
                        atoms
                            .lookup(&user_atoms::read_name(memory, pointer)?)
                            .unwrap_or(0),
                    ),
                };
                let title = if args[1] == 0 {
                    None
                } else {
                    Some(read_title(memory, args[1])?)
                };
                Ok(self
                    .top_levels
                    .iter()
                    .rev()
                    .find_map(|(&handle, window)| {
                        (class.is_none_or(|atom| atom == window.class)
                            && title
                                .as_ref()
                                .is_none_or(|title| title.eq_ignore_ascii_case(&window.title)))
                        .then_some(handle)
                    })
                    .unwrap_or(0))
            }
        }
    }
}

pub(super) fn read_title(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    if pointer == 0 {
        return Ok(String::new());
    }
    let mut title = String::new();
    for offset in 0..4096 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(title);
        }
        if offset == 4095 || !byte[0].is_ascii() {
            return Err(DispatchError::Unsupported);
        }
        title.push(char::from(byte[0]));
    }
    unreachable!()
}

impl super::Process32 {
    pub(super) fn window_query(&mut self, call: Call, args: &[u32]) -> Result<(), DispatchError> {
        let value = self
            .desktop
            .dispatch(call, args, &self.user_atoms, &self.memory)?;
        self.cpu.set_register(super::Register32::Eax, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    #[test]
    fn populated_registry_filters_owned_class_and_title_and_excludes_desktop() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x10000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x10000, b"DEMO\0").unwrap();
        memory.write(0x10020, b"TITLE\0").unwrap();
        memory.write(0x10040, b"unknown\0").unwrap();
        let mut atoms = UserAtoms::default();
        let Ok(atom) = atoms.retain_class("demo".into(), &mut memory) else {
            panic!("class atom allocation failed");
        };
        let mut desktop = Desktop::default();
        desktop.top_levels.insert(
            0x7500_0004,
            Window {
                class: atom,
                title: "title".into(),
                ..Window::default()
            },
        );
        desktop.top_levels.insert(
            0x7500_0008,
            Window {
                class: atom + 1,
                title: String::new(),
                ..Window::default()
            },
        );
        for (args, expected) in [
            ([0, 0], 0x7500_0008),
            ([0x10000, 0x10020], 0x7500_0004),
            ([atom, 0], 0x7500_0004),
            ([0, 0x10025], 0x7500_0008),
            ([atom + 1, 0x10020], 0),
            ([0x10040, 0], 0),
            ([0, 0x10040], 0),
        ] {
            assert!(
                matches!(desktop.dispatch(Call::Find, &args, &atoms, &memory), Ok(value) if value == expected)
            );
        }
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[0x7500_0004], &atoms, &memory),
            Ok(1)
        ));
        desktop.top_levels.clear();
        assert!(matches!(
            desktop.dispatch(Call::Find, &[0, 0], &atoms, &memory),
            Ok(0)
        ));
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[DESKTOP], &atoms, &memory),
            Ok(1)
        ));
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[0x7500_0004], &atoms, &memory),
            Ok(0)
        ));
    }
}
