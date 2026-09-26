use std::collections::{BTreeMap, BTreeSet};

use super::{Directory, DispatchError, LoadError, VirtualFile, paths};

const OUTPUT_BYTES_LIMIT: usize = 64 * 1024 * 1024;

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
    File(usize),
    Virtual(usize),
    Missing,
    Directory,
}

pub(in super::super) enum OutputTarget {
    New(Box<[u8]>),
    Existing(usize),
}

impl Directory {
    pub(in super::super) fn output_target(
        &self,
        input: &mut [u8],
    ) -> Result<Result<OutputTarget, u32>, DispatchError> {
        for byte in input.iter_mut() {
            if *byte == b'/' {
                *byte = b'\\';
            }
        }
        if input.len() >= 260 {
            return Err(DispatchError::Unsupported);
        }
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], input) {
            Ok(path) => path,
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
            Err(paths::PathError::Windows(_)) => return Ok(Err(2)),
        };
        if path.len() >= 260 {
            return Err(DispatchError::Unsupported);
        }
        if let Some(index) = self
            .created_files
            .iter()
            .position(|file| file.path.eq_ignore_ascii_case(&path))
        {
            return Ok(Ok(OutputTarget::Existing(index)));
        }
        if self
            .files
            .iter()
            .any(|file| !file.removed && file.path.eq_ignore_ascii_case(&path))
        {
            return Err(DispatchError::Unsupported);
        }
        if self.exists(&path) || input.last() == Some(&b'\\') {
            return Ok(Err(13));
        }
        let parent_end = path
            .iter()
            .rposition(|&byte| byte == b'\\')
            .expect("absolute path");
        if !self.exists(&path[..parent_end.max(3)]) {
            return Ok(Err(2));
        }
        if self.created_files.len() >= 256 {
            return Ok(Err(24));
        }
        Ok(Ok(OutputTarget::New(path.into_boxed_slice())))
    }

    pub(in super::super) fn publish_output(&mut self, target: OutputTarget) -> usize {
        match target {
            OutputTarget::New(path) => {
                let index = self.created_files.len();
                self.created_files.push(VirtualFile {
                    path,
                    bytes: Vec::new(),
                });
                index
            }
            OutputTarget::Existing(index) => {
                self.created_files[index].bytes.clear();
                index
            }
        }
    }

    pub(in super::super) fn output_len(&self, index: usize) -> usize {
        self.created_files[index].bytes.len()
    }

    pub(in super::super) fn output_contents(&self, index: usize) -> &[u8] {
        &self.created_files[index].bytes
    }

    pub(in super::super) fn write_output(
        &mut self,
        index: usize,
        position: usize,
        input: &[u8],
    ) -> bool {
        let Some(end) = position.checked_add(input.len()) else {
            return false;
        };
        let current = self.created_files[index].bytes.len();
        let new_len = current.max(end);
        let used: usize = self.created_files.iter().map(|file| file.bytes.len()).sum();
        if new_len > OUTPUT_BYTES_LIMIT || used > OUTPUT_BYTES_LIMIT - (new_len - current) {
            return false;
        }
        let file = &mut self.created_files[index].bytes;
        if file.try_reserve_exact(new_len - current).is_err() {
            return false;
        }
        file.resize(new_len, 0);
        file[position..end].copy_from_slice(input);
        true
    }

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
            if total > self.content_cache.limit() {
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
            self.files[index].contents = Some(bytes.into_boxed_slice());
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
            return if file.contents.is_some() || self.content_cache.on_demand() {
                Ok(ReadFile::File(index))
            } else {
                Err(DispatchError::Unsupported)
            };
        }
        if input.last() != Some(&b'\\')
            && let Some(index) = self
                .created_files
                .iter()
                .position(|file| file.path.eq_ignore_ascii_case(&path))
        {
            return Ok(ReadFile::Virtual(index));
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

#[cfg(test)]
mod tests {
    use super::{Directory, OutputTarget};

    #[test]
    fn virtual_output_writes_owned_bytes_with_zero_gap_and_truncation() {
        let mut directory = Directory::new(b"C:\\", &[b"C:\\savegames"], &[]).unwrap();
        let Ok(Ok(target)) = directory.output_target(&mut b"C:\\savegames\\slot.mps".to_vec())
        else {
            panic!("valid output target");
        };
        let index = directory.publish_output(target);
        assert!(directory.write_output(index, 5, b"abc"));
        assert_eq!(&directory.created_files[index].bytes, b"\0\0\0\0\0abc");
        assert!(directory.write_output(index, 6, b"XY"));
        assert_eq!(&directory.created_files[index].bytes, b"\0\0\0\0\0aXY");
        assert!(!directory.write_output(index, 64 * 1024 * 1024, b"x"));
        assert_eq!(&directory.created_files[index].bytes, b"\0\0\0\0\0aXY");
        directory.publish_output(OutputTarget::Existing(index));
        assert!(directory.created_files[index].bytes.is_empty());
    }
}
