use std::collections::BTreeMap;

use super::{
    DispatchError, GuestMemory, MemoryError, Register32, thread,
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
    Top,
    Window,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x10 => Some(Self::Desktop),
            0x26c => Some(Self::Find),
            0x270 => Some(Self::IsWindow),
            0x430 => Some(Self::Active),
            0x438 => Some(Self::DlgItem),
            0x454 => Some(Self::Top),
            0x458 => Some(Self::Window),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Desktop | Self::Active => 0,
            Self::IsWindow | Self::Top => 1,
            Self::Find | Self::DlgItem | Self::Window => 2,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowSnapshot {
    pub hwnd: u32,
    pub parent: u32,
    pub id: u32,
    pub class: u32,
    pub style: u32,
    pub exstyle: u32,
    pub title: String,
    pub rectangle: [i32; 4],
    pub client: [i32; 4],
    pub dialog_units: Option<[i16; 4]>,
    pub active: bool,
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
    pub(super) fn top_window(&self, parent: u32) -> u32 {
        if matches!(parent, 0 | DESKTOP) {
            return self.top_levels.keys().next_back().copied().unwrap_or(0);
        }
        self.children
            .iter()
            .rev()
            .find_map(|(&handle, window)| (window.parent == parent).then_some(handle))
            .unwrap_or(0)
    }

    pub(super) fn relative_window(&self, handle: u32, relation: u32) -> Option<u32> {
        let parent = self.window(handle)?.parent;
        if relation == 5 {
            return Some(self.top_window(handle));
        }
        if relation == 4 {
            return Some(0);
        }
        let siblings: Vec<u32> = if parent == 0 {
            self.top_levels.keys().copied().collect()
        } else {
            self.children
                .iter()
                .filter_map(|(&handle, window)| (window.parent == parent).then_some(handle))
                .collect()
        };
        let index = siblings.binary_search(&handle).ok()?;
        Some(match relation {
            0 => *siblings.last().expect("selected window exists"),
            1 => siblings[0],
            2 => index
                .checked_sub(1)
                .and_then(|index| siblings.get(index))
                .copied()
                .unwrap_or(0),
            3 => siblings.get(index + 1).copied().unwrap_or(0),
            _ => return None,
        })
    }

    pub(super) fn snapshots(&self) -> Vec<WindowSnapshot> {
        let mut snapshots: Vec<_> = self
            .top_levels
            .iter()
            .chain(&self.children)
            .map(|(&hwnd, window)| WindowSnapshot {
                hwnd,
                parent: window.parent,
                id: window.id,
                class: window.class,
                style: window.style,
                exstyle: window.exstyle,
                title: window.title.clone(),
                rectangle: window.rectangle,
                client: window.client,
                dialog_units: window.dialog_units,
                active: hwnd == self.active,
            })
            .collect();
        snapshots.sort_unstable_by_key(|window| window.hwnd);
        snapshots
    }

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
    pub(super) fn show_normal(&mut self, handle: u32) -> Option<u32> {
        let window = self.top_levels.get_mut(&handle)?;
        let was_visible = u32::from(window.style & 0x1000_0000 != 0);
        window.style |= 0x1000_0000;
        self.active = handle;
        Some(was_visible)
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
            Call::Top => Ok(self.top_window(args[0])),
            Call::Window => Ok(self.relative_window(args[0], args[1]).unwrap_or(0)),
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
        if matches!(call, Call::Top)
            && !matches!(args[0], 0 | DESKTOP)
            && self.desktop.window(args[0]).is_none()
        {
            thread::set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        }
        if matches!(call, Call::Window) {
            if args[1] > 5 {
                return Err(DispatchError::Unsupported);
            }
            if self.desktop.window(args[0]).is_none() {
                thread::set_last_error(&mut self.memory, 1400)?;
                self.cpu.set_register(Register32::Eax, 0);
                return Ok(());
            }
        }
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
    fn snapshots_include_children_in_handle_order_without_desktop_sentinel() {
        let mut desktop = Desktop::default();
        desktop.top_levels.insert(
            0x7500_0008,
            Window {
                title: "dialog".into(),
                ..Window::default()
            },
        );
        desktop.children.insert(
            0x7500_0004,
            Window {
                parent: 0x7500_0008,
                id: 42,
                class: 0x80,
                dialog_units: Some([1, 2, 3, 4]),
                ..Window::default()
            },
        );
        let snapshots = desktop.snapshots();
        assert_eq!(desktop.top_window(0x7500_0008), 0x7500_0004);
        assert_eq!(desktop.top_window(0), 0x7500_0008);
        assert_eq!(desktop.top_window(0x7500_0004), 0);
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].hwnd, 0x7500_0004);
        assert_eq!(snapshots[0].parent, 0x7500_0008);
        assert_eq!(snapshots[0].id, 42);
        assert_eq!(snapshots[0].dialog_units, Some([1, 2, 3, 4]));
        assert_eq!(snapshots[1].hwnd, 0x7500_0008);
        desktop.children.clear();
        assert_eq!(snapshots[0].class, 0x80);
        assert_eq!(snapshots[1].title, "dialog");
    }

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
