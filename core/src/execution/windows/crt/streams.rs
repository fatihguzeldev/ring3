use std::collections::BTreeMap;

use super::{
    Access, Cpu32, DispatchError, ERRNO, FMODE, GuestMemory, Register32,
    directory::{Directory, ReadFile},
    guest,
    heap::Heap,
    strings,
};

#[derive(Clone, Copy)]
pub(in super::super) enum Call {
    Open,
    Read,
    Close,
    Seek,
    Tell,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Open => 2,
            Self::Read => 4,
            Self::Close | Self::Tell => 1,
            Self::Seek => 3,
        }
    }
}

struct Stream {
    file: usize,
    position: usize,
    descriptor: u32,
    eof: bool,
}

impl Stream {
    fn record(&self) -> [u8; 32] {
        let mut bytes = [0; 32];
        let flags = 5_u32 | if self.eof { 0x10 } else { 0 };
        bytes[12..16].copy_from_slice(&flags.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.descriptor.to_le_bytes());
        bytes
    }
}

#[derive(Default)]
pub(super) struct Streams {
    live: BTreeMap<u32, Stream>,
}

impl Streams {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        cpu: &Cpu32,
        memory: &mut GuestMemory,
        heap: &mut Heap,
        directory: &mut Directory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Open => self.open(args, memory, heap, directory),
            Call::Read => self.read(args, memory, directory),
            Call::Close => {
                let file = self.validated(args[0], memory)?.file;
                heap.free_stream(args[0], cpu.register(Register32::Esp), memory)?;
                self.live.remove(&args[0]);
                directory.release_reader(file);
                Ok(0)
            }
            Call::Seek => self.seek(args, memory, directory),
            Call::Tell => self.tell(args[0], memory),
        }
    }

    fn open(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
        heap: &mut Heap,
        directory: &mut Directory,
    ) -> Result<u32, DispatchError> {
        if args[0] == 0 || args[1] == 0 {
            return Err(DispatchError::Unsupported);
        }
        let mode = strings::terminated_bytes(memory, args[1])?;
        match mode.as_slice() {
            b"rb\0" => {}
            b"r\0" => {
                let mut value = [0];
                guest::read_words(memory, FMODE, &mut value)?;
                if value[0] != 0x8000 {
                    return Err(DispatchError::Unsupported);
                }
            }
            _ => return Err(DispatchError::Unsupported),
        }
        let mut path = strings::terminated_bytes(memory, args[0])?;
        path.pop();
        if path.is_empty() || path.len() > 32767 {
            return Err(DispatchError::Unsupported);
        }
        let file = match directory.read_file(&mut path)? {
            ReadFile::Ready(index) => index,
            ReadFile::Missing => return failed(memory, 2),
            ReadFile::Directory => return failed(memory, 13),
        };
        let Some(descriptor) =
            (3..515).find(|id| self.live.values().all(|stream| stream.descriptor != *id))
        else {
            return failed(memory, 24);
        };
        let Some(pointer) = heap.allocate_stream(memory)? else {
            return failed(memory, 12);
        };
        let stream = Stream {
            file,
            position: 0,
            descriptor,
            eof: false,
        };
        memory
            .write(u64::from(pointer), &stream.record())
            .expect("new stream allocation is writable");
        directory.retain_reader(file);
        self.live.insert(pointer, stream);
        Ok(pointer)
    }

    fn validated(&self, pointer: u32, memory: &GuestMemory) -> Result<&Stream, DispatchError> {
        let stream = self.live.get(&pointer).ok_or(DispatchError::Unsupported)?;
        let mut actual = [0; 32];
        memory.read(u64::from(pointer), &mut actual)?;
        if actual != stream.record() {
            return Err(DispatchError::Unsupported);
        }
        Ok(stream)
    }

    fn read(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
        directory: &Directory,
    ) -> Result<u32, DispatchError> {
        if args[1] == 0 || args[2] == 0 {
            return Ok(0);
        }
        let requested = args[1]
            .checked_mul(args[2])
            .filter(|&n| n <= 64 * 1024 * 1024)
            .ok_or(DispatchError::Unsupported)? as usize;
        if args[0] == 0 {
            return Err(DispatchError::Unsupported);
        }
        let stream = self.validated(args[3], memory)?;
        let contents = directory.contents(stream.file);
        let source = contents.get(stream.position..).unwrap_or_default();
        let copied = requested.min(source.len());
        let pointer = u64::from(args[3]);
        let output = u64::from(args[0]);
        if copied != 0 && output < pointer + 32 && pointer < output + copied as u64 {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, args[0], copied, Access::Write)?;
        let eof = stream.eof || copied < requested;
        let changed = eof != stream.eof;
        if changed {
            guest::check(memory, args[3] + 12, 4, Access::Write)?;
        }
        memory.write(output, &source[..copied])?;
        if changed {
            guest::write_word(memory, args[3] + 12, 0x15)?;
        }
        let stream = self.live.get_mut(&args[3]).expect("validated stream");
        stream.position += copied;
        stream.eof = eof;
        Ok(u32::try_from(copied).expect("bounded read fits u32") / args[1])
    }

    fn seek(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
        directory: &Directory,
    ) -> Result<u32, DispatchError> {
        let stream = self.validated(args[0], memory)?;
        let base = match args[2] {
            0 => 0,
            1 => i64::try_from(stream.position).expect("stream position fits u32"),
            2 => i64::try_from(directory.contents(stream.file).len())
                .expect("file contents are bounded below i64"),
            _ => return invalid_position(memory),
        };
        let offset = i64::from(args[1].cast_signed());
        let Some(target) = base
            .checked_add(offset)
            .and_then(|value| u32::try_from(value).ok())
        else {
            return invalid_position(memory);
        };
        let clear_eof = stream.eof;
        if clear_eof {
            guest::check(memory, args[0] + 12, 4, Access::Write)?;
            guest::write_word(memory, args[0] + 12, 5)?;
        }
        let stream = self.live.get_mut(&args[0]).expect("validated stream");
        stream.position = target as usize;
        stream.eof = false;
        Ok(0)
    }

    fn tell(&self, pointer: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let position = u32::try_from(self.validated(pointer, memory)?.position)
            .expect("stream position fits u32");
        if position <= i32::MAX.cast_unsigned() {
            Ok(position)
        } else {
            invalid_position(memory)
        }
    }
}

fn failed(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    guest::write_word(memory, ERRNO, error)?;
    Ok(0)
}

fn invalid_position(memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    guest::write_word(memory, ERRNO, 22)?;
    Ok(u32::MAX)
}
