use super::{Access, DispatchError, GuestMemory, guest, multibyte::CodePage, strings};

impl CodePage {
    pub(super) fn split_path(
        self,
        memory: &mut GuestMemory,
        args: &[u32],
    ) -> Result<(), DispatchError> {
        if args[0] == 0 {
            return Err(DispatchError::Unsupported);
        }
        let source = match self {
            Self::Ansi | Self::Oem | Self::SingleByte => {
                strings::terminated_bytes(memory, args[0])?
            }
        };
        let path = &source[..source.len() - 1];
        let drive_end = if path.get(1) == Some(&b':') { 2 } else { 0 };
        let directory_end = path[drive_end..]
            .iter()
            .rposition(|byte| matches!(byte, b'/' | b'\\'))
            .map_or(drive_end, |offset| drive_end + offset + 1);
        let filename_end = path[directory_end..]
            .iter()
            .rposition(|byte| *byte == b'.')
            .map_or(path.len(), |offset| directory_end + offset);
        let parts = [
            &path[..drive_end],
            &path[drive_end..directory_end],
            &path[directory_end..filename_end],
            &path[filename_end..],
        ];
        let outputs = &args[1..5];
        for (index, (&address, part)) in outputs.iter().zip(parts).enumerate() {
            if address == 0 {
                continue;
            }
            let length = part.len() + 1;
            let limit = if index == 0 { 3 } else { 256 };
            if length > limit || overlap(address, length, args[0], source.len()) {
                return Err(DispatchError::Unsupported);
            }
            for (&prior, bytes) in outputs[..index].iter().zip(parts) {
                if prior != 0 && overlap(address, length, prior, bytes.len() + 1) {
                    return Err(DispatchError::Unsupported);
                }
            }
            guest::check(memory, address, length, Access::Write)?;
        }
        for (&address, part) in outputs.iter().zip(parts) {
            if address != 0 {
                memory.write(u64::from(address), part)?;
                memory.write(u64::from(address) + part.len() as u64, &[0])?;
            }
        }
        Ok(())
    }
}

fn overlap(left: u32, left_length: usize, right: u32, right_length: usize) -> bool {
    u64::from(left) < u64::from(right) + right_length as u64
        && u64::from(right) < u64::from(left) + left_length as u64
}
