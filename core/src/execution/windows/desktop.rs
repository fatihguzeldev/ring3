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
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x10 => Some(Self::Desktop),
            0x26c => Some(Self::Find),
            0x270 => Some(Self::IsWindow),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Desktop => 0,
            Self::IsWindow => 1,
            Self::Find => 2,
        }
    }
}

struct Window {
    class: u32,
    title: String,
}

#[derive(Default)]
pub(super) struct Desktop {
    top_levels: BTreeMap<u32, Window>,
}

impl Desktop {
    pub(super) fn dispatch(
        &self,
        call: Call,
        args: &[u32],
        atoms: &UserAtoms,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Desktop => Ok(DESKTOP),
            Call::IsWindow => Ok(u32::from(
                args[0] == DESKTOP || self.top_levels.contains_key(&args[0]),
            )),
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

fn read_title(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
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
            },
        );
        desktop.top_levels.insert(
            0x7500_0008,
            Window {
                class: atom + 1,
                title: String::new(),
            },
        );
        for (args, expected) in [
            ([0, 0], 0x7500_0004),
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
