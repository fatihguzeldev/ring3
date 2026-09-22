use std::collections::BTreeMap;

use super::super::Access;
use super::{DispatchError, GuestMemory, guest, modules::Modules, thread, user_atoms::UserAtoms};

const MAX_CLASSES: usize = 4096;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Query,
    Register,
    Unregister,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x260 => Some(Self::Query),
            0x264 => Some(Self::Register),
            0x268 => Some(Self::Unregister),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Query => 3,
            Self::Register => 1,
            Self::Unregister => 2,
        }
    }
}

#[derive(Default)]
pub(super) struct Classes {
    definitions: BTreeMap<(u32, u32), [u32; 9]>,
}

impl Classes {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        modules: &Modules,
        atoms: &mut UserAtoms,
        desktop: &super::desktop::Desktop,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Register) {
            return self.register(args[0], modules, atoms, memory);
        }
        let (instance, name) = if matches!(call, Call::Query) {
            (args[0], args[1])
        } else {
            (args[1], args[0])
        };
        let Some((atom, record)) = self.find(instance, name, modules, atoms, memory)? else {
            return failure(memory, 1411);
        };
        if matches!(call, Call::Query) {
            let mut bytes = [0; 40];
            for (i, word) in record.iter().chain(std::iter::once(&name)).enumerate() {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            guest::check(memory, args[2], bytes.len(), Access::Write)?;
            memory.write(u64::from(args[2]), &bytes)?;
            return Ok(atom);
        }
        if desktop.has_class(instance, atom) {
            return failure(memory, 1412);
        }
        self.definitions.remove(&(instance, atom));
        atoms.release_class(atom);
        Ok(1)
    }

    pub(super) fn find(
        &self,
        instance: u32,
        name: u32,
        modules: &Modules,
        atoms: &UserAtoms,
        memory: &GuestMemory,
    ) -> Result<Option<(u32, [u32; 9])>, DispatchError> {
        if instance == 0 || !modules.contains(instance) {
            return Err(DispatchError::Unsupported);
        }
        let atom = if name <= 0xffff {
            if name < 0xc000 {
                return Err(DispatchError::Unsupported);
            }
            name
        } else {
            let Some(atom) = atoms.lookup(&read_name(memory, name)?) else {
                return Ok(None);
            };
            atom
        };
        Ok(self
            .definitions
            .get(&(instance, atom))
            .map(|record| (atom, *record)))
    }

    fn register(
        &mut self,
        pointer: u32,
        modules: &Modules,
        atoms: &mut UserAtoms,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let mut record = [0; 10];
        guest::read_words(memory, pointer, &mut record)?;
        if record[0] & !0xb != 0
            || record[1] == 0
            || record[4] == 0
            || !modules.contains(record[4])
            || [
                record[2], record[3], record[5], record[6], record[7], record[8],
            ]
            .iter()
            .any(|&value| value != 0)
        {
            return Err(DispatchError::Unsupported);
        }
        let name = read_name(memory, record[9])?;
        if atoms
            .lookup(&name)
            .is_some_and(|atom| self.definitions.contains_key(&(record[4], atom)))
        {
            return failure(memory, 1410);
        }
        if self.definitions.len() == MAX_CLASSES {
            return failure(memory, 8);
        }
        let atom = atoms.retain_class(name, memory)?;
        if atom != 0 {
            self.definitions.insert(
                (record[4], atom),
                record[..9].try_into().expect("fixed class record"),
            );
        }
        Ok(atom)
    }
}

fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    let name = super::user_atoms::read_name(memory, pointer)?;
    if [
        "button",
        "combobox",
        "edit",
        "listbox",
        "mdiclient",
        "scrollbar",
        "static",
        "combolbox",
        "ddemlevent",
        "message",
    ]
    .contains(&name.as_str())
    {
        return Err(DispatchError::Unsupported);
    }
    Ok(name)
}

fn failure(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(0)
}

impl super::Process32 {
    pub(super) fn window_class(
        &mut self,
        call: Call,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        let value = self.classes.dispatch(
            call,
            arguments,
            &self.modules,
            &mut self.user_atoms,
            &self.desktop,
            &mut self.memory,
        )?;
        self.cpu.set_register(super::Register32::Eax, value);
        Ok(())
    }
}
