use std::collections::{BTreeMap, BTreeSet};

use super::{Directory, DispatchError, LoadError, paths};

/// bytes copied into the process for an existing declared file.
#[derive(Clone, Copy)]
pub struct FileContents<'a> {
    pub path: &'a [u8],
    pub bytes: &'a [u8],
}

impl std::fmt::Debug for FileContents<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileContents")
            .field("path", &self.path)
            .field("byte_length", &self.bytes.len())
            .finish()
    }
}

pub(in super::super) enum ReadFile {
    Ready(usize),
    Missing,
    Directory,
}

impl Directory {
    pub(in super::super) fn attach_contents(
        &mut self,
        inputs: &[FileContents<'_>],
    ) -> Result<(), LoadError> {
        if inputs.len() > self.files.len() {
            return Err(LoadError::InvalidProcessParameters);
        }
        let indices: BTreeMap<_, _> = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.to_ascii_lowercase(), index))
            .collect();
        let mut seen = BTreeSet::new();
        let mut total = 0_usize;
        for input in inputs {
            if input.path.len() > 32767 {
                return Err(LoadError::InvalidProcessParameters);
            }
            let key = input.path.to_ascii_lowercase();
            let Some(&index) = indices.get(&key) else {
                return Err(LoadError::InvalidProcessParameters);
            };
            if !seen.insert(key) || input.bytes.len() as u64 != self.files[index].size {
                return Err(LoadError::InvalidProcessParameters);
            }
            total = total
                .checked_add(input.bytes.len())
                .ok_or(LoadError::FileContentsLimitExceeded)?;
            if total > 1024 * 1024 * 1024 {
                return Err(LoadError::FileContentsLimitExceeded);
            }
        }
        for input in inputs {
            let index = indices[&input.path.to_ascii_lowercase()];
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(input.bytes.len())
                .map_err(|_| LoadError::FileContentsAllocationFailed)?;
            bytes.extend_from_slice(input.bytes);
            self.files[index].contents = Some(bytes);
        }
        Ok(())
    }

    pub(in super::super) fn read_file(&self, input: &mut [u8]) -> Result<ReadFile, DispatchError> {
        for byte in input.iter_mut() {
            if *byte == b'/' {
                *byte = b'\\';
            }
        }
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], input) {
            Ok(path) => path,
            Err(paths::PathError::Windows(_)) => return Ok(ReadFile::Missing),
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
        };
        if input.last() != Some(&b'\\')
            && let Some((index, file)) = self
                .files
                .iter()
                .enumerate()
                .find(|(_, file)| !file.removed && file.path.eq_ignore_ascii_case(&path))
        {
            return if file.contents.is_some() {
                Ok(ReadFile::Ready(index))
            } else {
                Err(DispatchError::Unsupported)
            };
        }
        Ok(if self.exists(&path) {
            ReadFile::Directory
        } else {
            ReadFile::Missing
        })
    }

    pub(in super::super) fn contents(&self, index: usize) -> &[u8] {
        self.files[index]
            .contents
            .as_deref()
            .expect("opened file has supplied contents")
    }

    pub(in super::super) fn retain_reader(&mut self, index: usize) {
        self.files[index].readers += 1;
    }

    pub(in super::super) fn release_reader(&mut self, index: usize) {
        self.files[index].readers -= 1;
    }
}
