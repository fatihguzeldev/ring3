use super::SecurityError32 as Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sid32 {
    authority: u64,
    subauthorities: [u32; 15],
    count: u8,
}
impl Sid32 {
    pub(crate) fn new(authority: u64, subauthorities: &[u32]) -> Result<Self, Error> {
        if authority >> 48 != 0 || subauthorities.len() > 15 {
            return Err(Error::InvalidSid);
        }
        let mut value = Self {
            authority,
            subauthorities: [0; 15],
            count: subauthorities.len() as u8,
        };
        value.subauthorities[..subauthorities.len()].copy_from_slice(subauthorities);
        Ok(value)
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
            return Err(Error::InvalidSid);
        }
        let count = bytes[1] as usize;
        if bytes.len() < 8 + count * 4 {
            return Err(Error::InvalidSid);
        }
        let authority = bytes[2..8]
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte));
        let mut subauthorities = [0; 15];
        for (index, value) in subauthorities[..count].iter_mut().enumerate() {
            *value = u32::from_le_bytes(bytes[8 + index * 4..12 + index * 4].try_into().unwrap());
        }
        Self::new(authority, &subauthorities[..count])
    }
    pub(crate) fn byte_len(self) -> usize {
        8 + usize::from(self.count) * 4
    }
    pub(crate) fn write(self, output: &mut [u8]) -> Result<usize, Error> {
        let length = self.byte_len();
        if output.len() < length {
            return Err(Error::InvalidParameter);
        }
        output[0] = 1;
        output[1] = self.count;
        output[2..8].copy_from_slice(&self.authority.to_be_bytes()[2..]);
        for (index, value) in self.subauthorities[..usize::from(self.count)]
            .iter()
            .enumerate()
        {
            output[8 + index * 4..12 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(length)
    }
    pub(crate) fn everyone() -> Self {
        Self::new(1, &[0]).unwrap()
    }
    pub(crate) fn owner_rights() -> Self {
        Self::new(3, &[4]).unwrap()
    }
    pub(crate) fn creator_owner() -> Self {
        Self::new(3, &[0]).unwrap()
    }
    pub(crate) fn creator_group() -> Self {
        Self::new(3, &[1]).unwrap()
    }
    pub(crate) fn system() -> Self {
        Self::new(5, &[18]).unwrap()
    }
    pub(crate) fn virtual_user() -> Self {
        Self::new(5, &[21, 0x5233, 0, 0, 1000]).unwrap()
    }
}
