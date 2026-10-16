use crate::windows::process::ProcessError32;

pub(super) struct StartupBytes32 {
    pub(super) base: u32,
    pub(super) bytes: Vec<u8>,
    pub(super) used: usize,
}
impl StartupBytes32 {
    pub(super) fn new(base: u32, length: usize) -> Self {
        Self {
            base,
            bytes: vec![0; length],
            used: 0x300,
        }
    }
    pub(super) fn put(&mut self, bytes: &[u8], alignment: usize) -> Result<u32, ProcessError32> {
        let start = self.used.div_ceil(alignment) * alignment;
        let end = start
            .checked_add(bytes.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or(ProcessError32::Capacity)?;
        self.bytes[start..end].copy_from_slice(bytes);
        self.used = end;
        Ok(self.base + start as u32)
    }
    pub(super) fn wide(&mut self, value: &str) -> Result<(u32, u16), ProcessError32> {
        self.wide_units(&value.encode_utf16().collect::<Vec<_>>())
    }
    pub(super) fn wide_units(&mut self, units: &[u16]) -> Result<(u32, u16), ProcessError32> {
        let bytes = wide_bytes(units)?;
        Ok((self.put(&bytes, 2)?, (bytes.len() - 2) as u16))
    }
    pub(super) fn word(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    pub(super) fn unicode(&mut self, offset: usize, value: (u32, u16)) {
        self.bytes[offset..offset + 2].copy_from_slice(&value.1.to_le_bytes());
        self.bytes[offset + 2..offset + 4].copy_from_slice(&(value.1 + 2).to_le_bytes());
        self.word(offset + 4, value.0);
    }
}

pub(super) fn wide_bytes(units: &[u16]) -> Result<Vec<u8>, ProcessError32> {
    if units.len() > 32766 || units.contains(&0) {
        return Err(ProcessError32::InvalidParameter);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact((units.len() + 1) * 2)
        .map_err(|_| ProcessError32::Allocation)?;
    bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    bytes.extend_from_slice(&[0, 0]);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_unicode_is_pe32_and_capacity_checked() {
        let mut bytes = StartupBytes32::new(0x7000_0000, 4096);
        let value = bytes.wide("A😀").unwrap();
        bytes.unicode(0x40, value);
        assert_eq!(&bytes.bytes[0x40..0x48], &[6, 0, 8, 0, 0, 3, 0, 0x70]);
        assert_eq!(
            &bytes.bytes[0x300..0x308],
            &[65, 0, 0x3d, 0xd8, 0, 0xde, 0, 0]
        );
        let before = bytes.used;
        assert_eq!(bytes.put(&vec![0; 4096], 4), Err(ProcessError32::Capacity));
        assert_eq!(bytes.used, before);
    }
}
