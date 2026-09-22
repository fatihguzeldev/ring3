use super::super::Access;
use super::{
    API_BASE, Api, DispatchError, GuestMemory, MemoryError, guest, parameters, system, thread,
};
use crate::execution::loader::modules::MappedModule;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Load,
    Handle,
    Free,
    FileName,
    DisableThreadCalls,
    Procedure,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::FileName => 3,
            Self::Procedure => 2,
            _ => 1,
        }
    }

    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x14 => Some(Self::Load),
            0x18 => Some(Self::Handle),
            0x1c => Some(Self::Free),
            0xe0 => Some(Self::FileName),
            0xfc => Some(Self::DisableThreadCalls),
            0x274 => Some(Self::Procedure),
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
    thread_notifications: bool,
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
                thread_notifications: true,
            })
            .collect();
        // builtin handles are opaque identities in the reserved api page, not images.
        for (name, offset) in [
            ("kernel32.dll", 0x800),
            ("msvcrt.dll", 0x804),
            ("d3d8.dll", 0x808),
            ("user32.dll", 0x80c),
            ("gdi32.dll", 0x810),
            ("winmm.dll", 0x814),
        ] {
            if !resident
                .iter()
                .any(|module| module.name.eq_ignore_ascii_case(name))
            {
                resident.push(Module {
                    name: name.to_owned(),
                    path: [system::SYSTEM_DIRECTORY, b"\\", name.as_bytes(), &[0]].concat(),
                    handle: API_BASE + offset,
                    references: 1,
                    builtin: true,
                    thread_notifications: true,
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
        if matches!(call, Call::Procedure) {
            return self.procedure(arguments[0], arguments[1], memory);
        }
        let argument = arguments[0];
        if matches!(call, Call::DisableThreadCalls) {
            if argument == self.program {
                return Err(DispatchError::Unsupported);
            }
            let Some(module) = self
                .resident
                .iter_mut()
                .find(|module| module.handle == argument)
            else {
                thread::set_last_error(memory, 126)?;
                return Ok(0);
            };
            module.thread_notifications = false;
            return Ok(1);
        }
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
        let absolute = name.as_bytes().get(1) == Some(&b':');
        if absolute && matches_path(&self.program_path, &name) {
            if matches!(call, Call::Load)
                || self
                    .resident
                    .iter()
                    .any(|module| matches_path(&module.path, &name))
            {
                return Err(DispatchError::Unsupported);
            }
            return Ok(self.program);
        }
        let Some(module) = self.resident.iter_mut().find(|module| {
            if absolute {
                matches_path(&module.path, &name)
            } else {
                module.name.eq_ignore_ascii_case(&name)
            }
        }) else {
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

    fn procedure(
        &self,
        handle: u32,
        pointer: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if handle == 0 || handle == self.program {
            return Err(DispatchError::Unsupported);
        }
        let Some(module) = self.resident.iter().find(|module| module.handle == handle) else {
            thread::set_last_error(memory, 126)?;
            return Ok(0);
        };
        if !module.builtin || pointer <= 0xffff {
            return Err(DispatchError::Unsupported);
        }
        let name = procedure_name(memory, pointer)?;
        Api::resolve_name(&module.name, &name).ok_or(DispatchError::Unsupported)
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

fn matches_path(path: &[u8], name: &str) -> bool {
    path[..path.len() - 1].eq_ignore_ascii_case(name.as_bytes())
}

fn read_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    let mut name = String::new();
    for offset in 0..=32767 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        let absolute = name.as_bytes().get(1) == Some(&b':');
        if byte[0] == 0 {
            if name.is_empty()
                || matches!(name.as_str(), "." | "..")
                || (absolute && !parameters::valid_absolute_path(name.as_bytes()))
            {
                return Err(DispatchError::Unsupported);
            }
            if name.ends_with('.') {
                name.pop();
            } else if !name
                .rsplit('\\')
                .next()
                .expect("nonempty name")
                .contains('.')
            {
                name.push_str(".dll");
            }
            if absolute && !parameters::valid_absolute_path(name.as_bytes()) {
                return Err(DispatchError::Unsupported);
            }
            return Ok(name);
        }
        let drive_colon =
            offset == 1 && name.as_bytes()[0].is_ascii_alphabetic() && byte[0] == b':';
        let valid = if absolute {
            (offset != 2 || byte[0] == b'\\')
                && (0x20..=0x7e).contains(&byte[0])
                && !b"<>:\"|?*/".contains(&byte[0])
        } else {
            drive_colon || byte[0].is_ascii_alphanumeric() || matches!(byte[0], b'.' | b'_' | b'-')
        };
        if offset == if absolute { 32767 } else { 255 } || !valid {
            return Err(DispatchError::Unsupported);
        }
        name.push(char::from(byte[0]));
    }
    unreachable!()
}

fn procedure_name(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    let mut name = Vec::new();
    for offset in 0..4096_u32 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 && !name.is_empty() {
            return Ok(String::from_utf8(name).expect("validated ascii export name"));
        }
        if !(0x20..=0x7e).contains(&byte[0]) {
            return Err(DispatchError::Unsupported);
        }
        name.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{PAGE_SIZE, Permissions};

    #[test]
    fn notification_policy_is_per_module_and_per_process_without_reference_changes() {
        let create = || {
            Modules::new(
                0x0040_0000,
                b"C:\\program.exe",
                vec![
                    MappedModule {
                        name: "first.dll".into(),
                        base: 0x5000_0000,
                    },
                    MappedModule {
                        name: "second.dll".into(),
                        base: 0x5001_0000,
                    },
                ],
            )
        };
        let mut modules = create();
        let other = create();
        let mut memory = GuestMemory::new(0);
        for _ in 0..2 {
            assert_eq!(
                modules
                    .dispatch(Call::DisableThreadCalls, &[0x5000_0000], false, &mut memory)
                    .ok(),
                Some(1)
            );
            assert!(!modules.resident[0].thread_notifications);
            assert!(
                modules.resident[1..]
                    .iter()
                    .all(|module| module.thread_notifications)
            );
            assert!(modules.resident.iter().all(|module| module.references == 1));
            assert!(
                other
                    .resident
                    .iter()
                    .all(|module| module.thread_notifications)
            );
        }
        assert!(matches!(
            modules.dispatch(Call::DisableThreadCalls, &[0], true, &mut memory),
            Err(DispatchError::Memory(_))
        ));
        assert!(!modules.resident[0].thread_notifications);
        assert!(modules.resident[1].thread_notifications);
    }

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
