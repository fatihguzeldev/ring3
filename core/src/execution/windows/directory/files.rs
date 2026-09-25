use std::collections::BTreeMap;

use super::{Directory, DispatchError, GuestMemory, paths, thread};

const FIRST: u32 = 0x7a00_0004;
const LAST: u32 = 0x7aff_fffc;
const MAX_LIVE: usize = 4096;

pub(super) struct Opened {
    // stable file index and opening flags; no guest pointers are retained.
    live: BTreeMap<u32, (usize, u32)>,
    next: u32,
}

impl Default for Opened {
    fn default() -> Self {
        Self {
            live: BTreeMap::new(),
            next: FIRST,
        }
    }
}

impl Directory {
    pub(super) fn open_file(
        &mut self,
        args: &[u32],
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if args[1] != 0x8000_0000
            || args[2] != 1
            || args[3] != 0
            || args[4] != 3
            || !matches!(args[5], 0 | 0x80 | 0x2000_0000 | 0x2000_0080)
        {
            return Err(DispatchError::Unsupported);
        }
        let mut input = paths::read(memory, args[0])?;
        for byte in &mut input {
            if *byte == b'/' {
                *byte = b'\\';
            }
        }
        let path = match paths::resolve(&self.terminated[..self.terminated.len() - 1], &input) {
            Ok(path) => path,
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
            Err(paths::PathError::Windows(error)) => return failed(teb, memory, error),
        };
        let index = self.files.iter().position(|file| {
            !file.removed && input.last() != Some(&b'\\') && file.path.eq_ignore_ascii_case(&path)
        });
        let Some(index) = index else {
            let error = if self.exists(&path) {
                5
            } else {
                let end = path
                    .iter()
                    .rposition(|&b| b == b'\\')
                    .expect("absolute file path")
                    .max(3);
                if input.last() == Some(&b'\\') || !self.exists(&path[..end]) {
                    3
                } else {
                    2
                }
            };
            return failed(teb, memory, error);
        };
        if self.files[index].contents.is_none() {
            return Err(DispatchError::Unsupported);
        }
        if self.opened.live.len() == MAX_LIVE || self.opened.next > LAST {
            return failed(teb, memory, 8);
        }
        let handle = self.opened.next;
        self.opened.live.insert(handle, (index, args[5]));
        self.opened.next += 4;
        self.retain_reader(index);
        Ok(handle)
    }

    pub(super) fn close_file(&mut self, handle: u32) -> bool {
        let Some((index, _flags)) = self.opened.live.remove(&handle) else {
            return false;
        };
        self.release_reader(index);
        true
    }
}

fn failed(teb: thread::Teb, memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    teb.set_last_error(memory, error)?;
    Ok(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{FileContents, FileMetadata, Permissions};

    fn setup() -> (Directory, GuestMemory, thread::Teb) {
        let mut directory = Directory::new(
            b"C:\\",
            &[],
            &[FileMetadata {
                path: b"C:\\sample.bin",
                size: 4,
            }],
        )
        .unwrap();
        directory
            .attach_contents(&[FileContents {
                path: b"C:\\sample.bin",
                bytes: b"data",
            }])
            .unwrap();
        let mut memory = GuestMemory::new(4);
        memory
            .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1000, b"sample.bin\0").unwrap();
        thread::initialize(&mut memory, 0, 0).unwrap();
        (directory, memory, thread::Teb(thread::BASE))
    }

    #[test]
    fn final_identity_retains_flags_and_failed_exhaustion_does_not_change_pins() {
        for flags in [0, 0x80, 0x2000_0000, 0x2000_0080] {
            let (mut directory, mut memory, teb) = setup();
            directory.opened.next = LAST;
            let args = [0x1000, 0x8000_0000, 1, 0, 3, flags, u32::MAX];
            assert!(matches!(
                directory.open_file(&args, teb, &mut memory),
                Ok(LAST)
            ));
            assert_eq!(directory.opened.live.get(&LAST), Some(&(0, flags)));
            assert_eq!(directory.files[0].readers, 1);
            memory
                .protect(u64::from(thread::BASE), 4096, Permissions::NONE)
                .unwrap();
            assert!(matches!(
                directory.open_file(&args, teb, &mut memory),
                Err(DispatchError::Memory(_))
            ));
            assert_eq!(directory.opened.next, LAST + 4);
            assert_eq!(directory.files[0].readers, 1);
            assert_eq!(directory.opened.live.len(), 1);
            memory
                .protect(u64::from(thread::BASE), 4096, Permissions::READ_WRITE)
                .unwrap();
            assert!(matches!(
                directory.open_file(&args, teb, &mut memory),
                Ok(u32::MAX)
            ));
            assert_eq!(teb.last_error(&memory).unwrap(), 8);
            assert!(directory.close_file(LAST));
            assert!(!directory.close_file(LAST));
            assert_eq!(directory.files[0].readers, 0);
            assert!(matches!(
                directory.open_file(&args, teb, &mut memory),
                Ok(u32::MAX)
            ));
            assert!(directory.opened.live.is_empty());
        }
    }

    #[test]
    fn opening_retains_owned_content_identity_after_path_unmap() {
        let (mut directory, mut memory, teb) = setup();
        let mut input = b"next".to_vec();
        directory
            .attach_contents(&[FileContents {
                path: b"C:\\sample.bin",
                bytes: &input,
            }])
            .unwrap();
        input.fill(b'x');
        assert!(matches!(
            directory.open_file(
                &[0x1000, 0x8000_0000, 1, 0, 3, 0x2000_0080, 0],
                teb,
                &mut memory
            ),
            Ok(FIRST)
        ));
        memory.unmap(0x1000, 4096).unwrap();
        assert_eq!(directory.contents(0), b"next");
        assert_eq!(directory.opened.live.get(&FIRST), Some(&(0, 0x2000_0080)));
        assert_eq!(directory.files[0].readers, 1);
        assert!(directory.close_file(FIRST));
        assert_eq!(directory.files[0].readers, 0);
        assert_eq!(directory.contents(0), b"next");
    }
}
