use super::{API_BASE, DispatchError, GuestMemory, MemoryError, thread};
use crate::execution::loader::modules::MappedModule;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Load,
    Handle,
    Free,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x14 => Some(Self::Load),
            0x18 => Some(Self::Handle),
            0x1c => Some(Self::Free),
            _ => None,
        }
    }
}

struct Module {
    name: String,
    handle: u32,
    references: u32,
    builtin: bool,
}

pub(super) struct Modules {
    program: u32,
    resident: Vec<Module>,
}

impl Modules {
    pub(super) fn new(program: u32, providers: Vec<MappedModule>) -> Self {
        let mut resident: Vec<_> = providers
            .into_iter()
            .map(|provider| Module {
                name: provider.name,
                handle: provider.base,
                references: 1,
                builtin: false,
            })
            .collect();
        // builtin handles are opaque identities in the reserved api page, not images.
        for (name, offset) in [
            ("kernel32.dll", 0x800),
            ("msvcrt.dll", 0x804),
            ("d3d8.dll", 0x808),
            ("user32.dll", 0x80c),
        ] {
            if !resident
                .iter()
                .any(|module| module.name.eq_ignore_ascii_case(name))
            {
                resident.push(Module {
                    name: name.to_owned(),
                    handle: API_BASE + offset,
                    references: 1,
                    builtin: true,
                });
            }
        }
        Self { program, resident }
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        argument: u32,
        initialized: bool,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Free) {
            let Some(module) = self
                .resident
                .iter_mut()
                .find(|module| module.handle == argument)
            else {
                thread::set_last_error(memory, 6)?;
                return Ok(0);
            };
            if module.references == 1 {
                // the final release requires detach/unload, which is not implemented.
                return Err(DispatchError::Unsupported);
            }
            module.references -= 1;
            return Ok(1);
        }
        if matches!(call, Call::Handle) && argument == 0 {
            return Ok(self.program);
        }
        let name = read_name(memory, argument)?;
        let Some(module) = self
            .resident
            .iter_mut()
            .find(|module| module.name.eq_ignore_ascii_case(&name))
        else {
            thread::set_last_error(memory, 126)?;
            return Ok(0);
        };
        if matches!(call, Call::Load) {
            if !module.builtin && !initialized {
                return Err(DispatchError::Unsupported);
            }
            module.references = module
                .references
                .checked_add(1)
                .ok_or(DispatchError::Unsupported)?;
        }
        Ok(module.handle)
    }
}

fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    let mut name = String::new();
    for offset in 0..=255 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            if name.is_empty() || matches!(name.as_str(), "." | "..") {
                return Err(DispatchError::Unsupported);
            }
            if name.ends_with('.') {
                name.pop();
            } else if !name.contains('.') {
                name.push_str(".dll");
            }
            return Ok(name);
        }
        if offset == 255
            || !(byte[0].is_ascii_alphanumeric() || matches!(byte[0], b'.' | b'_' | b'-'))
        {
            return Err(DispatchError::Unsupported);
        }
        name.push(char::from(byte[0]));
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{PAGE_SIZE, Permissions};

    #[test]
    fn reference_overflow_does_not_mutate_state() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1000, b"kernel32\0").unwrap();
        let mut modules = Modules::new(0x0040_0000, Vec::new());
        modules.resident[0].references = u32::MAX;
        assert!(matches!(
            modules.dispatch(Call::Load, 0x1000, true, &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(modules.resident[0].references, u32::MAX);
    }
}
