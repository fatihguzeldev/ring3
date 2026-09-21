use super::super::Access;
use super::{
    DispatchError, GuestMemory, LoadError, MemoryError, Process32, Register32, guest, parameters,
    thread,
};

mod paths;

pub(super) struct Directory {
    terminated: Vec<u8>,
    declarations: Vec<Box<[u8]>>,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Query,
    Change,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Query => 2,
            Self::Change => 1,
        }
    }
}

impl Process32 {
    pub(super) fn directory(&mut self, call: Call, arguments: &[u32]) -> Result<(), DispatchError> {
        let result = match call {
            Call::Query => {
                self.current_directory
                    .query(arguments[0], arguments[1], &mut self.memory)?
            }
            Call::Change => self
                .current_directory
                .change(arguments[0], &mut self.memory)?,
        };
        self.cpu.set_register(Register32::Eax, result);
        Ok(())
    }
}

impl Directory {
    pub(super) fn new(path: &[u8], directories: &[&[u8]]) -> Result<Self, LoadError> {
        if directories.len() >= 256 {
            return Err(LoadError::InvalidProcessParameters);
        }
        let entries = || std::iter::once(path).chain(directories.iter().copied());
        let mut bytes = 0;
        for entry in entries() {
            let root = entry.len() == 3 && entry[0].is_ascii_alphabetic() && &entry[1..] == b":\\";
            if !root && !parameters::valid_absolute_path(entry) {
                return Err(LoadError::InvalidProcessParameters);
            }
            bytes += entry.len() + 1;
            if bytes > 65536 {
                return Err(LoadError::InvalidProcessParameters);
            }
        }
        let declarations = entries().map(Box::from).collect();
        let mut terminated = Vec::with_capacity(path.len() + 1);
        terminated.extend_from_slice(path);
        terminated.push(0);
        Ok(Self {
            terminated,
            declarations,
        })
    }

    fn change(&mut self, source: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let input = paths::read(memory, source)?;
        let mut candidate =
            match paths::resolve(&self.terminated[..self.terminated.len() - 1], &input) {
                Ok(candidate) => candidate,
                Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
                Err(paths::PathError::Windows(error)) => return failed(memory, error),
            };
        let exists = self.declarations.iter().any(|declared| {
            declared
                .get(..candidate.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&candidate))
                && (candidate.len() == declared.len()
                    || candidate.len() == 3
                    || declared.get(candidate.len()) == Some(&b'\\'))
        });
        if !exists {
            return failed(memory, 3);
        }
        candidate.push(0);
        self.terminated = candidate;
        Ok(1)
    }

    pub(super) fn query(
        &self,
        capacity: u32,
        output: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let required = u32::try_from(self.terminated.len()).expect("bounded directory");
        if capacity < required {
            return Ok(required);
        }
        guest::check(memory, output, self.terminated.len(), Access::Write)?;
        memory.write(u64::from(output), &self.terminated)?;
        Ok(required - 1)
    }
}

fn failed(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(0)
}
