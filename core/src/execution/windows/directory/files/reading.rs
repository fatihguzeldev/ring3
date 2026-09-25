use super::super::{Access, guest, volume};
use super::{Directory, DispatchError, GuestMemory, thread};

impl Directory {
    pub(in super::super) fn read_open_file(
        &mut self,
        args: &[u32],
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if args[4] != 0 || args[3] == 0 || args[2] > 64 * 1024 * 1024 {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, args[3], 4, Access::Write)?;
        let Some(file) = self.opened.live.get(&args[0]) else {
            return failed(teb, memory, args[3], 6);
        };
        if file.flags & 0x2000_0000 != 0
            && args[2] != 0
            && [file.position, args[1], args[2]]
                .iter()
                .any(|n| !n.is_multiple_of(volume::SECTOR_BYTES))
        {
            return failed(teb, memory, args[3], 87);
        }
        let requested = args[2] as usize;
        if requested != 0 {
            guest::check(memory, args[1], requested, Access::Write)?;
        }
        let source = self
            .contents(file.index)
            .get(file.position as usize..)
            .unwrap_or_default();
        let copied = requested.min(source.len());
        if copied != 0 {
            memory.write(u64::from(args[1]), &source[..copied])?;
        }
        let copied = u32::try_from(copied).expect("bounded read fits u32");
        guest::write_word(memory, args[3], copied)?;
        self.opened
            .live
            .get_mut(&args[0])
            .expect("validated file handle")
            .position += copied;
        Ok(1)
    }
}

fn failed(
    teb: thread::Teb,
    memory: &mut GuestMemory,
    count: u32,
    error: u32,
) -> Result<u32, DispatchError> {
    teb.check_last_error_write(memory)?;
    guest::write_word(memory, count, 0)?;
    teb.set_last_error(memory, error)?;
    Ok(0)
}
