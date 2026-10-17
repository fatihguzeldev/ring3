use super::document::{ProfileDocument32, integer};
use crate::{
    files::{
        FileAccess32, FileDisposition32, FileError32, FileIo32, FileOpenOptions32, FileShare32,
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory},
    },
    windows::{files::FileApi32, security::Token32},
};
impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn profile_call(
        &mut self,
        memory_space: &mut AddressSpace,
        token: &Token32,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        let read_string = |pointer| -> Result<Option<String>, FileError32> {
            if pointer == 0 {
                return Ok(None);
            }
            let units = memory::path(memory_space, pointer, false, self.ordinary.ansi)?;
            Ok(Some(String::from_utf16_lossy(&units)))
        };
        let section = read_string(args[0])?;
        let key = read_string(args[1])?;
        let writing = api == WritePrivateProfileStringA;
        let data = if writing || api == GetPrivateProfileStringA {
            read_string(args[2])?
        } else {
            None
        };
        if writing && section.is_none() && key.is_none() && data.is_none() {
            return Ok(FileCallOutcome32::success(0));
        }
        if writing && section.is_none() {
            return Err(FileError32::InvalidParameter);
        }
        let filename = if api == GetProfileIntA {
            None
        } else {
            read_string(
                args[if api == GetPrivateProfileStringA {
                    5
                } else {
                    3
                }],
            )?
        };
        let filename = filename.as_deref().unwrap_or("win.ini");
        let mut path = if filename.contains(['\\', '/', ':']) {
            filename.encode_utf16().collect::<Vec<_>>()
        } else {
            let mut path = self.virtual_windows_directory().to_vec();
            path.push(92);
            path.extend(filename.encode_utf16());
            path
        };
        path = self.vfs.full_path(&path)?;
        if api == GetPrivateProfileStringA {
            memory::output(memory_space, args[3], args[4] as usize)?;
        }
        let info = match self.vfs.path_info(&path) {
            Ok(info) => Some(info),
            Err(FileError32::NotFound | FileError32::PathNotFound) => None,
            Err(error) => return Err(error),
        };
        let mut source = Vec::new();
        if let Some(info) = info {
            if info.size > self.vfs.max_io_bytes() as u64 {
                return Err(FileError32::Capacity);
            }
            let prepared = self.vfs.prepare_open_with_security(
                &path,
                FileOpenOptions32 {
                    access: FileAccess32::READ,
                    share: FileShare32::ALL,
                    disposition: FileDisposition32::OpenExisting,
                    ..Default::default()
                },
                0x80000000,
                token,
                None,
            )?;
            prepared.validate(&self.vfs)?;
            let opened = self.vfs.commit_open(prepared);
            self.vfs.check_close_open(opened.open)?;
            let read = self.vfs.prepare_read_at(info.id, 0, info.size as usize);
            let result = match read {
                Ok(FileIo32::NeedBytes(request)) => Ok(Some(request)),
                Ok(FileIo32::Ready(read)) => {
                    source = read.bytes().to_vec();
                    self.vfs.commit_read(read);
                    Ok(None)
                }
                Err(error) => Err(error),
            };
            self.vfs.close_open(opened.open)?;
            if let Some(request) = result? {
                return Ok(FileCallOutcome32::PendingBytes(request));
            }
        }
        let wide = source.starts_with(&[0xff, 0xfe]);
        let text = if wide {
            String::from_utf16_lossy(
                &source[2..]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
        } else {
            String::from_utf16_lossy(
                &self
                    .ordinary
                    .ansi
                    .decode(&source, false)
                    .map_err(|_| FileError32::InvalidName)?,
            )
        };
        let mut document = ProfileDocument32::new(&text);
        if writing {
            document.set(section.as_deref().unwrap(), key.as_deref(), data.as_deref());
            if let Some(info) = info
                && self.mappings.minimum_file_size(info.id) > 0
            {
                return Err(FileError32::UserMappedFile);
            }
            let text = document.text();
            let bytes = if wide {
                [
                    vec![0xff, 0xfe],
                    text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                ]
                .concat()
            } else {
                self.ordinary
                    .ansi
                    .encode(&text.encode_utf16().collect::<Vec<_>>(), false)
                    .map_err(|_| FileError32::InvalidName)?
                    .0
            };
            self.vfs.replace_provider_file(&path, &bytes, token)?;
            return Ok(FileCallOutcome32::success(1));
        }
        if api == GetPrivateProfileIntA || api == GetProfileIntA {
            let value = section
                .as_deref()
                .zip(key.as_deref())
                .and_then(|(section, key)| document.value(section, key));
            return Ok(FileCallOutcome32::success(
                value
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map_or(args[2], integer),
            ));
        }
        let multiple = section.is_none() || key.is_none();
        let mut bytes = if multiple {
            let mut result = Vec::new();
            for name in document.names(section.as_deref()) {
                result.extend(
                    self.ordinary
                        .ansi
                        .encode(&name.encode_utf16().collect::<Vec<_>>(), false)
                        .map_err(|_| FileError32::InvalidName)?
                        .0,
                );
                result.push(0);
            }
            if result.is_empty() {
                result.push(0);
            }
            result.push(0);
            result
        } else {
            let value = document
                .value(section.as_deref().unwrap(), key.as_deref().unwrap())
                .unwrap_or_else(|| data.unwrap_or_default().trim_end_matches(' ').to_owned());
            let mut result = self
                .ordinary
                .ansi
                .encode(&value.encode_utf16().collect::<Vec<_>>(), false)
                .map_err(|_| FileError32::InvalidName)?
                .0;
            result.push(0);
            result
        };
        let capacity = args[4] as usize;
        if capacity == 0 {
            return Ok(FileCallOutcome32::success(0));
        }
        let truncated = bytes.len() > capacity;
        bytes.truncate(capacity);
        let count = if multiple && truncated {
            let count = capacity.saturating_sub(2);
            bytes[count..].fill(0);
            count
        } else {
            let count = if multiple && bytes == [0, 0] {
                0
            } else {
                bytes.len() - 1
            };
            let last = bytes.len() - 1;
            bytes[last] = 0;
            count
        };
        memory_space
            .write(GuestAddress(args[3]), &bytes)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(FileCallOutcome32::Return {
            value: count as u32,
            last_error: info.is_none().then_some(2),
        })
    }
}
