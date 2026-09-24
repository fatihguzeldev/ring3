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
    state: State,
}

enum State {
    Ready,
    Deferred { entry: Option<u32> },
    Initializing { entry: u32 },
}

impl Module {
    fn visible(&self) -> bool {
        matches!(self.state, State::Ready | State::Initializing { .. })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Pending {
    pub(super) handle: u32,
    pub(super) entry: u32,
}

pub(super) enum Load {
    Complete(u32),
    Initialize(Pending),
}

pub(super) struct Modules {
    program: u32,
    program_path: Vec<u8>,
    resident: Vec<Module>,
}

impl Modules {
    pub(super) fn load(
        &mut self,
        argument: u32,
        initialized: bool,
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<Load, DispatchError> {
        let name = read_name(memory, argument)?;
        let absolute = name.as_bytes().get(1) == Some(&b':');
        if absolute && matches_path(&self.program_path, &name) {
            return Err(DispatchError::Unsupported);
        }
        let Some(module) = self.resident.iter_mut().find(|module| {
            if absolute {
                matches_path(&module.path, &name)
            } else {
                module.name.eq_ignore_ascii_case(&name)
            }
        }) else {
            teb.set_last_error(memory, 126)?;
            return Ok(Load::Complete(0));
        };
        match module.state {
            State::Ready => {
                if !module.builtin && !initialized {
                    return Err(DispatchError::Unsupported);
                }
                module.references = module
                    .references
                    .checked_add(1)
                    .ok_or(DispatchError::Unsupported)?;
                Ok(Load::Complete(module.handle))
            }
            State::Deferred { entry: None } if initialized => {
                module.state = State::Ready;
                module.references = 1;
                Ok(Load::Complete(module.handle))
            }
            State::Deferred { entry: Some(entry) } if initialized => {
                Ok(Load::Initialize(Pending {
                    handle: module.handle,
                    entry,
                }))
            }
            State::Deferred { .. } | State::Initializing { .. } => Err(DispatchError::Unsupported),
        }
    }

    pub(super) fn start(&mut self, pending: Pending) {
        let module = self
            .resident
            .iter_mut()
            .find(|module| module.handle == pending.handle)
            .expect("prepared deferred module is retained");
        debug_assert!(matches!(
            module.state,
            State::Deferred {
                entry: Some(entry)
            } if entry == pending.entry
        ));
        module.state = State::Initializing {
            entry: pending.entry,
        };
    }

    pub(super) fn finish(&mut self, pending: Pending, success: bool) {
        let module = self
            .resident
            .iter_mut()
            .find(|module| module.handle == pending.handle)
            .expect("initializing module is retained");
        debug_assert!(matches!(
            module.state,
            State::Initializing { entry } if entry == pending.entry
        ));
        if success {
            module.state = State::Ready;
            module.references = 1;
        } else {
            module.state = State::Deferred {
                entry: Some(pending.entry),
            };
        }
    }

    pub(super) fn contains(&self, handle: u32) -> bool {
        handle == self.program
            || self
                .resident
                .iter()
                .any(|module| module.handle == handle && module.visible())
    }

    pub(super) fn new(
        program: u32,
        program_path: &[u8],
        providers: Vec<MappedModule>,
        startup_count: usize,
        deferred_initializers: &[crate::execution::loader::modules::Initializer],
    ) -> Self {
        let parent_end = program_path
            .iter()
            .rposition(|&byte| byte == b'\\')
            .expect("validated absolute path")
            + 1;
        let parent = &program_path[..parent_end];
        let mut resident: Vec<_> = providers
            .into_iter()
            .enumerate()
            .map(|(index, provider)| Module {
                state: if index < startup_count {
                    State::Ready
                } else {
                    State::Deferred {
                        entry: deferred_initializers
                            .iter()
                            .find(|initializer| initializer.name == provider.name)
                            .map(|initializer| initializer.entry),
                    }
                },
                path: [parent, provider.name.as_bytes(), &[0]].concat(),
                name: provider.name,
                handle: provider.base,
                references: u32::from(index < startup_count),
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
            ("advapi32.dll", 0x818),
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
                    state: State::Ready,
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
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Load) {
            return match self.load(arguments[0], initialized, teb, memory)? {
                Load::Complete(value) => Ok(value),
                Load::Initialize(_) => Err(DispatchError::Unsupported),
            };
        }
        if matches!(call, Call::Procedure) {
            return self.procedure(arguments[0], arguments[1], teb, memory);
        }
        let argument = arguments[0];
        if matches!(call, Call::DisableThreadCalls) {
            if argument == self.program {
                return Err(DispatchError::Unsupported);
            }
            let Some(module) = self
                .resident
                .iter_mut()
                .find(|module| module.handle == argument && module.visible())
            else {
                teb.set_last_error(memory, 126)?;
                return Ok(0);
            };
            module.thread_notifications = false;
            return Ok(1);
        }
        if matches!(call, Call::FileName) {
            return self.file_name(argument, arguments[1], arguments[2], teb, memory);
        }
        if matches!(call, Call::Free) {
            let Some(module) = self
                .resident
                .iter_mut()
                .find(|module| module.handle == argument && module.visible())
            else {
                teb.set_last_error(memory, 6)?;
                return Ok(0);
            };
            if matches!(module.state, State::Initializing { .. }) {
                return Err(DispatchError::Unsupported);
            }
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
            if self
                .resident
                .iter()
                .any(|module| matches_path(&module.path, &name))
            {
                return Err(DispatchError::Unsupported);
            }
            return Ok(self.program);
        }
        let Some(module) = self.resident.iter_mut().find(|module| {
            if !module.visible() {
                return false;
            }
            if absolute {
                matches_path(&module.path, &name)
            } else {
                module.name.eq_ignore_ascii_case(&name)
            }
        }) else {
            teb.set_last_error(memory, 126)?;
            return Ok(0);
        };
        Ok(module.handle)
    }

    fn procedure(
        &self,
        handle: u32,
        pointer: u32,
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if handle == 0 || handle == self.program {
            return Err(DispatchError::Unsupported);
        }
        let Some(module) = self
            .resident
            .iter()
            .find(|module| module.handle == handle && module.visible())
        else {
            teb.set_last_error(memory, 126)?;
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
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let path = if handle == 0 || handle == self.program {
            &self.program_path
        } else if let Some(module) = self
            .resident
            .iter()
            .find(|module| module.handle == handle && module.visible())
        {
            &module.path
        } else {
            teb.set_last_error(memory, 126)?;
            return Ok(0);
        };
        let count = (size as usize).min(path.len());
        let truncated = (size as usize) < path.len();
        guest::check(memory, output, count, Access::Write)?;
        if truncated {
            teb.check_last_error_write(memory)?;
        }
        memory.write(u64::from(output), &path[..count])?;
        if truncated {
            teb.set_last_error(memory, 0)
                .expect("last-error output was checked");
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
                2,
                &[],
            )
        };
        let mut modules = create();
        let other = create();
        let mut memory = GuestMemory::new(0);
        for _ in 0..2 {
            assert_eq!(
                modules
                    .dispatch(
                        Call::DisableThreadCalls,
                        &[0x5000_0000],
                        false,
                        thread::Teb(thread::BASE),
                        &mut memory
                    )
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
            modules.dispatch(
                Call::DisableThreadCalls,
                &[0],
                true,
                thread::Teb(thread::BASE),
                &mut memory
            ),
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
        let mut modules = Modules::new(0x0040_0000, b"C:\\program.exe", Vec::new(), 0, &[]);
        modules.resident[0].references = u32::MAX;
        assert!(matches!(
            modules.dispatch(
                Call::Load,
                &[0x1000],
                true,
                thread::Teb(thread::BASE),
                &mut memory
            ),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(modules.resident[0].references, u32::MAX);
    }

    #[test]
    fn initializing_module_cannot_release_its_unpublished_reference() {
        let mut memory = GuestMemory::new(1);
        let mut modules = Modules::new(0x0040_0000, b"C:\\program.exe", Vec::new(), 0, &[]);
        modules.resident[0].state = State::Initializing { entry: 1 };
        modules.resident[0].references = 0;
        let handle = modules.resident[0].handle;
        assert!(matches!(
            modules.dispatch(
                Call::Free,
                &[handle],
                true,
                thread::Teb(thread::BASE),
                &mut memory
            ),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(modules.resident[0].references, 0);
    }
}
