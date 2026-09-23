use super::super::Access;
use super::{
    DispatchError, GuestMemory, LoadError, MemoryError, Process32, Register32, guest, parameters,
    thread,
};

mod catalog;
mod contents;
mod paths;
mod search;
pub use catalog::FileMetadata;
pub use contents::FileContents;
pub(super) use contents::ReadFile;

pub(super) struct Directory {
    terminated: Vec<u8>,
    declarations: Vec<Box<[u8]>>,
    files: Vec<catalog::File>,
    searches: search::Searches,
}

pub(super) struct Status {
    pub(super) drive: u8,
    pub(super) size: u64,
    pub(super) directory: bool,
    pub(super) executable: bool,
}

pub(super) enum Removal {
    Removed,
    Missing,
    Directory,
    Open,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Query,
    Change,
    FindFirst,
    FindNext,
    FindClose,
    Attributes,
    ShortPath,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::ShortPath => 3,
            Self::Query | Self::FindFirst | Self::FindNext => 2,
            Self::Change | Self::FindClose | Self::Attributes => 1,
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
            Call::FindFirst => {
                self.current_directory
                    .find_first(arguments[0], arguments[1], &mut self.memory)?
            }
            Call::FindNext => {
                self.current_directory
                    .find_next(arguments[0], arguments[1], &mut self.memory)?
            }
            Call::FindClose => self
                .current_directory
                .find_close(arguments[0], &mut self.memory)?,
            Call::Attributes => self
                .current_directory
                .attributes(arguments[0], &mut self.memory)?,
            Call::ShortPath => self.current_directory.short_path(
                arguments[0],
                arguments[1],
                arguments[2],
                &mut self.memory,
            )?,
        };
        self.cpu.set_register(Register32::Eax, result);
        Ok(())
    }
}

impl Directory {
    pub(super) fn has_file(&self, input: &[u8]) -> Result<bool, DispatchError> {
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], input) {
            Ok(path) => path,
            Err(paths::PathError::Windows(_) | paths::PathError::Unsupported) => {
                return Err(DispatchError::Unsupported);
            }
        };
        Ok(self
            .files
            .iter()
            .any(|file| !file.removed && file.path.eq_ignore_ascii_case(&path)))
    }

    pub(super) fn new(
        path: &[u8],
        directories: &[&[u8]],
        files: &[FileMetadata<'_>],
    ) -> Result<Self, LoadError> {
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
        let declarations: Vec<Box<[u8]>> = entries().map(Box::from).collect();
        let files = catalog::prepare(files, &declarations)?;
        let mut terminated = Vec::with_capacity(path.len() + 1);
        terminated.extend_from_slice(path);
        terminated.push(0);
        Ok(Self {
            terminated,
            declarations,
            files,
            searches: search::Searches::default(),
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
        let exists = self.exists(&candidate);
        if !exists {
            return failed(memory, 3);
        }
        candidate.push(0);
        self.terminated = candidate;
        Ok(1)
    }

    fn all_paths(&self) -> impl Iterator<Item = &[u8]> {
        self.declarations
            .iter()
            .map(AsRef::as_ref)
            .chain(self.files.iter().map(|file| file.path.as_ref()))
    }

    fn exists(&self, candidate: &[u8]) -> bool {
        self.all_paths().enumerate().any(|(index, declared)| {
            declared
                .get(..candidate.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(candidate))
                && ((candidate.len() == declared.len() && index < self.declarations.len())
                    || candidate.len() == 3
                    || declared.get(candidate.len()) == Some(&b'\\'))
        })
    }

    fn attributes(&self, source: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let input = paths::read(memory, source)?;
        self.path_attributes(&input, memory)
    }

    fn path_attributes(
        &self,
        input: &[u8],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if input.len() >= 260 {
            return Err(DispatchError::Unsupported);
        }
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], input) {
            Ok(path) => path,
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
            Err(paths::PathError::Windows(error)) => {
                return failed(memory, error).map(|_| u32::MAX);
            }
        };
        if path.len() >= 260 {
            return Err(DispatchError::Unsupported);
        }
        if self
            .files
            .iter()
            .any(|file| !file.removed && file.path.eq_ignore_ascii_case(&path))
        {
            if input.last() == Some(&b'\\') {
                return Err(DispatchError::Unsupported);
            }
            return Ok(128);
        }
        if self.exists(&path) {
            return Ok(16);
        }
        let parent_end = path
            .iter()
            .rposition(|&b| b == b'\\')
            .expect("absolute path");
        let error = if self.exists(&path[..parent_end.max(3)]) {
            2
        } else {
            3
        };
        failed(memory, error).map(|_| u32::MAX)
    }

    fn short_path(
        &self,
        source: u32,
        output: u32,
        capacity: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let mut input = paths::read(memory, source)?;
        if input.is_empty() {
            return failed(memory, 161);
        }
        if self.path_attributes(&input, memory)? == u32::MAX {
            return Ok(0);
        }
        let required = u32::try_from(input.len() + 1).expect("bounded path");
        if capacity < required {
            return Ok(required);
        }
        input.push(0);
        guest::check(memory, output, input.len(), Access::Write)?;
        memory.write(u64::from(output), &input)?;
        Ok(required - 1)
    }

    pub(super) fn legacy_status(
        &self,
        memory: &GuestMemory,
        source: u32,
    ) -> Result<Option<Status>, DispatchError> {
        let input = paths::read(memory, source)?;
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], &input) {
            Ok(path) => path,
            Err(paths::PathError::Windows(_)) => return Ok(None),
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
        };
        if input.last() == Some(&b'\\')
            && input.len() != 1
            && !(input.len() == 3 && input[1] == b':')
        {
            return Ok(None);
        }
        let file = self
            .files
            .iter()
            .find(|file| !file.removed && file.path.eq_ignore_ascii_case(&path));
        if file.is_none() && !self.exists(&path) {
            return Ok(None);
        }
        let executable = [b".exe", b".com", b".bat", b".cmd"]
            .iter()
            .any(|extension| {
                path.get(path.len().saturating_sub(4)..)
                    .is_some_and(|end| end.eq_ignore_ascii_case(*extension))
            });
        Ok(Some(Status {
            drive: path[0].to_ascii_uppercase() - b'A',
            size: file.map_or(0, |file| file.size),
            directory: file.is_none(),
            executable,
        }))
    }

    pub(super) fn remove_file(
        &mut self,
        memory: &GuestMemory,
        source: u32,
    ) -> Result<Removal, DispatchError> {
        let input = paths::read(memory, source)?;
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], &input) {
            Ok(path) => path,
            Err(paths::PathError::Windows(_)) => return Ok(Removal::Missing),
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
        };
        if let Some(file) = self
            .files
            .iter_mut()
            .find(|file| file.path.eq_ignore_ascii_case(&path))
        {
            if file.removed || input.last() == Some(&b'\\') {
                return Ok(Removal::Missing);
            }
            if file.readers != 0 {
                return Ok(Removal::Open);
            }
            // retain path scaffolding and stable indices for directory and search snapshots.
            file.removed = true;
            return Ok(Removal::Removed);
        }
        Ok(if self.exists(&path) {
            Removal::Directory
        } else {
            Removal::Missing
        })
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
