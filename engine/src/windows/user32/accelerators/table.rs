use crate::windows::user32::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Accelerator32 {
    pub flags: u8,
    pub key: u16,
    pub command: u16,
}

impl Accelerator32 {
    pub(crate) fn resource(bytes: &[u8], limit: usize) -> Result<Vec<Self>, E> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(8) {
            return Err(E::InvalidRequest);
        }
        let count = bytes.len() / 8;
        if count > limit {
            return Err(E::Capacity);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        for (index, bytes) in bytes.chunks_exact(8).enumerate() {
            let flags = u16::from_le_bytes([bytes[0], bytes[1]]);
            if flags & !0xff != 0 || (flags & 0x80 != 0) != (index + 1 == count) {
                return Err(E::InvalidRequest);
            }
            entries.push(Self {
                flags: (flags & 0x7f) as u8,
                key: u16::from_le_bytes([bytes[2], bytes[3]]),
                command: u16::from_le_bytes([bytes[4], bytes[5]]),
            });
        }
        Ok(entries)
    }

    pub(crate) fn api(bytes: &[u8], limit: usize) -> Result<Vec<Self>, E> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(6) {
            return Err(E::Win32(87));
        }
        let count = bytes.len() / 6;
        if count > limit {
            return Err(E::Capacity);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        for bytes in bytes.chunks_exact(6) {
            entries.push(Self {
                flags: bytes[0] & 0x7f,
                key: u16::from_le_bytes([bytes[2], bytes[3]]),
                command: u16::from_le_bytes([bytes[4], bytes[5]]),
            });
        }
        Ok(entries)
    }

    pub(crate) fn encode(self) -> [u8; 6] {
        let key = self.key.to_le_bytes();
        let command = self.command.to_le_bytes();
        [self.flags, 0, key[0], key[1], command[0], command[1]]
    }
}
