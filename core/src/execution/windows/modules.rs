use super::super::Access;
use super::{API_BASE, DispatchError, GuestMemory, MemoryError, guest, thread};
use crate::execution::loader::modules::MappedModule;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Load,
    Handle,
    Free,
    FileName,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        if matches!(self, Self::FileName) { 3 } else { 1 }
    }

    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x14 => Some(Self::Load),
            0x18 => Some(Self::Handle),
            0x1c => Some(Self::Free),
            0xe0 => Some(Self::FileName),
            _ => None,
        }
    }
}

struct Module {
    name: String,
    path: Vec<u8>,
    handle: u32,
    references: u32,
    builtin: bool,
}

pub(super) struct Modules {
    program: u32,
    program_path: Vec<u8>,
    resident: Vec<Module>,
}

impl Modules {
    pub(super) fn contains(&self, handle: u32) -> bool {
        handle == self.program || self.resident.iter().any(|module| module.handle == handle)
    }

    pub(super) fn new(program: u32, program_path: &[u8], providers: Vec<MappedModule>) -> Self {
        let parent_end = program_path
            .iter()
            .rposition(|&byte| byte == b'\\')
            .expect("validated absolute path")
            + 1;
        let parent = &program_path[..parent_end];
        let mut resident: Vec<_> = providers
            .into_iter()
            .map(|provider| Module {
                path: [parent, provider.name.as_bytes(), &[0]].concat(),
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
            ("gdi32.dll", 0x810),
        ] {
            if !resident
                .iter()
                .any(|module| module.name.eq_ignore_ascii_case(name))
            {
                resident.push(Module {
                    name: name.to_owned(),
                    path: [b"C:\\Windows\\System32\\".as_slice(), name.as_bytes(), &[0]].concat(),
                    handle: API_BASE + offset,
                    references: 1,
                    builtin: true,
                });
            }
        }
        Self {
            program,
            program_path: [program_path, &[0]].concat(),
            resident,
        }
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        initialized: bool,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let argument = arguments[0];
        if matches!(call, Call::FileName) {
            return self.file_name(argument, arguments[1], arguments[2], memory);
        }
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

    fn file_name(
        &self,
        handle: u32,
        output: u32,
        size: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let path = if handle == 0 || handle == self.program {
            &self.program_path
        } else if let Some(module) = self.resident.iter().find(|module| module.handle == handle) {
            &module.path
        } else {
            thread::set_last_error(memory, 126)?;
            return Ok(0);
        };
        let count = (size as usize).min(path.len());
        let truncated = (size as usize) < path.len();
        guest::check(memory, output, count, Access::Write)?;
        if truncated {
            thread::check_last_error_write(memory)?;
        }
        memory.write(u64::from(output), &path[..count])?;
        if truncated {
            thread::set_last_error(memory, 0).expect("last-error output was checked");
            Ok(size)
        } else {
            Ok(u32::try_from(path.len() - 1).expect("bounded image path"))
        }
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
        let mut modules = Modules::new(0x0040_0000, b"C:\\program.exe", Vec::new());
        modules.resident[0].references = u32::MAX;
        assert!(matches!(
            modules.dispatch(Call::Load, &[0x1000], true, &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(modules.resident[0].references, u32::MAX);
    }
}
