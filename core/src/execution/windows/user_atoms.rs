use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, MemoryError, Register32, thread};

impl super::Process32 {
    pub(super) fn register_user_atom(&mut self, pointer: u32) -> Result<(), DispatchError> {
        let value =
            self.user_atoms
                .register(pointer, thread::Teb(self.cpu.fs_base()), &mut self.memory)?;
        self.cpu.set_register(Register32::Eax, value);
        Ok(())
    }
}

pub(super) struct UserAtoms {
    names: BTreeMap<String, Atom>,
    next: u32,
}

struct Atom {
    identifier: u32,
    classes: usize,
    pinned: bool,
}

impl Default for UserAtoms {
    fn default() -> Self {
        Self {
            names: BTreeMap::new(),
            next: 0xc000,
        }
    }
}

impl UserAtoms {
    pub(super) fn register(
        &mut self,
        pointer: u32,
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        self.retain(read_name(memory, pointer)?, true, teb, memory)
    }

    pub(super) fn lookup(&self, name: &str) -> Option<u32> {
        self.names.get(name).map(|entry| entry.identifier)
    }

    pub(super) fn retain_class(
        &mut self,
        name: String,
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        self.retain(name, false, teb, memory)
    }

    pub(super) fn release_class(&mut self, atom: u32) {
        self.names.retain(|_, entry| {
            if entry.identifier == atom {
                entry.classes -= 1;
            }
            entry.pinned || entry.classes != 0
        });
    }

    fn retain(
        &mut self,
        name: String,
        pin: bool,
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if let Some(entry) = self.names.get_mut(&name) {
            entry.pinned |= pin;
            entry.classes += usize::from(!pin);
            return Ok(entry.identifier);
        }
        if self.next > 0xffff {
            teb.set_last_error(memory, 8)?;
            return Ok(0);
        }
        let identifier = self.next;
        self.names.insert(
            name,
            Atom {
                identifier,
                classes: usize::from(!pin),
                pinned: pin,
            },
        );
        self.next += 1;
        Ok(identifier)
    }
}

pub(super) fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    // integer atom pointers require a separate contract from string names.
    if pointer <= 0xffff {
        return Err(DispatchError::Unsupported);
    }
    let mut name = String::new();
    for offset in 0..=255 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return if name.is_empty() {
                Err(DispatchError::Unsupported)
            } else {
                Ok(name)
            };
        }
        if offset == 255 || !byte[0].is_ascii() || (offset == 0 && byte[0] == b'#') {
            return Err(DispatchError::Unsupported);
        }
        name.push(char::from(byte[0].to_ascii_lowercase()));
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    #[test]
    fn atom_capacity_uses_caller_error_without_losing_class_references() {
        let mut memory = GuestMemory::new(2);
        thread::initialize(&mut memory, 0, 0).unwrap();
        thread::set_last_error(&mut memory, 77).unwrap();
        let teb = thread::Teb(0x1101_0000);
        memory
            .map_zeroed(u64::from(teb.0), 4096, Permissions::READ_WRITE)
            .unwrap();
        thread::initialize_contents(&mut memory, teb.0, 2, 0, 0).unwrap();
        let mut atoms = UserAtoms {
            next: 0xffff,
            ..UserAtoms::default()
        };
        assert!(matches!(
            atoms.retain_class("demo".into(), teb, &mut memory),
            Ok(0xffff)
        ));
        memory
            .protect(u64::from(teb.0), 4096, Permissions::READ)
            .unwrap();
        assert!(matches!(
            atoms.retain_class("other".into(), teb, &mut memory),
            Err(DispatchError::Memory(_))
        ));
        assert_eq!(atoms.next, 0x10000);
        assert_eq!(atoms.lookup("other"), None);
        assert!(matches!(
            atoms.retain_class("demo".into(), teb, &mut memory),
            Ok(0xffff)
        ));
        memory
            .protect(u64::from(teb.0), 4096, Permissions::READ_WRITE)
            .unwrap();
        assert!(matches!(
            atoms.retain_class("other".into(), teb, &mut memory),
            Ok(0)
        ));
        assert_eq!(teb.last_error(&memory).unwrap(), 8);
        assert_eq!(thread::last_error(&memory).unwrap(), 77);
        atoms.release_class(0xffff);
        assert_eq!(atoms.lookup("demo"), Some(0xffff));
        atoms.release_class(0xffff);
        assert_eq!(atoms.lookup("demo"), None);
        assert_eq!(atoms.next, 0x10000);
    }

    #[test]
    fn exhausted_identities_keep_shared_names_and_never_recycle_retired_atoms() {
        for pin in [false, true] {
            let mut memory = GuestMemory::new(1);
            thread::initialize(&mut memory, 0, 0).unwrap();
            let mut atoms = UserAtoms {
                next: 0xffff,
                ..UserAtoms::default()
            };
            for _ in 0..2 {
                assert!(matches!(
                    atoms.retain_class("demo".into(), thread::Teb(thread::BASE), &mut memory),
                    Ok(0xffff)
                ));
            }
            memory
                .protect(0x7ffd_e000, 4096, Permissions::NONE)
                .unwrap();
            assert!(
                atoms
                    .retain_class("other".into(), thread::Teb(thread::BASE), &mut memory)
                    .is_err()
            );
            assert_eq!(atoms.lookup("other"), None);
            if pin {
                assert!(matches!(
                    atoms.retain("demo".into(), true, thread::Teb(thread::BASE), &mut memory),
                    Ok(0xffff)
                ));
            }
            atoms.release_class(0xffff);
            assert_eq!(atoms.lookup("demo"), Some(0xffff));
            atoms.release_class(0xffff);
            assert_eq!(atoms.lookup("demo"), pin.then_some(0xffff));
            memory
                .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
                .unwrap();
            assert!(matches!(
                atoms.retain_class("other".into(), thread::Teb(thread::BASE), &mut memory),
                Ok(0)
            ));
            assert_eq!(thread::last_error(&memory).unwrap(), 8);
            let expected = if pin { 0xffff } else { 0 };
            assert!(matches!(
                atoms.retain_class("demo".into(), thread::Teb(thread::BASE), &mut memory),
                Ok(value) if value == expected
            ));
            assert_eq!(atoms.next, 0x10000);
        }
    }
}
