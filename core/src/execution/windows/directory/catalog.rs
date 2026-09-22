use std::collections::BTreeSet;

use super::{LoadError, parameters, paths};

/// initial file metadata; no file contents or host filesystem access are implied.
#[derive(Clone, Copy, Debug)]
pub struct FileMetadata<'a> {
    pub path: &'a [u8],
    pub size: u64,
}

pub(super) struct File {
    pub(super) path: Box<[u8]>,
    pub(super) size: u64,
    pub(super) removed: bool,
    pub(super) contents: Option<Vec<u8>>,
    pub(super) readers: u16,
}

pub(super) fn prepare(
    files: &[FileMetadata<'_>],
    directories: &[Box<[u8]>],
) -> Result<Vec<File>, LoadError> {
    if files.len() > 4096 {
        return Err(LoadError::InvalidProcessParameters);
    }
    let mut bytes = 0;
    for file in files {
        if !parameters::valid_absolute_path(file.path)
            || file.path[3..]
                .split(|&b| b == b'\\')
                .any(|part| part.len() > 259 || paths::validate_component(part).is_err())
        {
            return Err(LoadError::InvalidProcessParameters);
        }
        bytes += file.path.len() + 1;
        if bytes > 1024 * 1024 {
            return Err(LoadError::InvalidProcessParameters);
        }
    }
    let mut keys = BTreeSet::new();
    for file in files {
        if !keys.insert(file.path.to_ascii_lowercase()) {
            return Err(LoadError::InvalidProcessParameters);
        }
    }
    for (path, directory) in files
        .iter()
        .map(|file| (file.path, false))
        .chain(directories.iter().map(|path| (path.as_ref(), true)))
    {
        let key = path.to_ascii_lowercase();
        if (directory && keys.contains(&key))
            || key
                .iter()
                .enumerate()
                .skip(3)
                .any(|(end, &byte)| byte == b'\\' && keys.contains(&key[..end]))
        {
            return Err(LoadError::InvalidProcessParameters);
        }
    }
    Ok(files
        .iter()
        .map(|file| File {
            path: file.path.into(),
            size: file.size,
            removed: false,
            contents: None,
            readers: 0,
        })
        .collect())
}
