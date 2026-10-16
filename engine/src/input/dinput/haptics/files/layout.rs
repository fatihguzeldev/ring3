use crate::input::dinput::DirectInputResult32 as H;

// layout cross-checked against our authored windows ffe data; provenance lives
// beside the test fixtures. custom data preserves its raw pe32 pointer.
pub(super) const FORMAT: [u8; 16] = [
    0x5c, 0x77, 0x7e, 0x19, 0xba, 0x34, 0xd3, 0x11, 0xab, 0xd5, 0, 0xc0, 0x4f, 0x8e, 0xc6, 0x27,
];
pub(super) struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}
impl<'a> Cursor<'a> {
    pub fn take(&mut self, length: usize) -> Result<&'a [u8], H> {
        let end = self.at.checked_add(length).ok_or(H::INVALID_PARAM)?;
        let bytes = self.bytes.get(self.at..end).ok_or(H::INVALID_PARAM)?;
        self.at = end;
        Ok(bytes)
    }
    pub fn word(&mut self) -> Result<u32, H> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn data(&mut self) -> Result<&'a [u8], H> {
        if self.take(4)? != b"data" {
            return Err(H::INVALID_PARAM);
        }
        let length = self.word()? as usize;
        let data = self.take(length)?;
        self.take(length & 1)?;
        Ok(data)
    }
}
pub(super) fn copied(bytes: &[u8]) -> Result<Vec<u8>, H> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes.len())
        .map_err(|_| H::OUT_OF_MEMORY)?;
    result.extend_from_slice(bytes);
    Ok(result)
}
