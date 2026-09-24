use super::{DispatchError, GuestMemory, INVALID_ARGUMENT, guest};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BufferSize {
    pub(super) requested: u32,
    pub(super) capacity: u32,
}

impl BufferSize {
    pub(super) fn set(
        &mut self,
        property: u32,
        address: u32,
        memory: &GuestMemory,
        acquired: bool,
    ) -> Result<u32, DispatchError> {
        if property != 1 {
            return Err(DispatchError::Unsupported);
        }
        if address == 0 {
            return Ok(INVALID_ARGUMENT);
        }
        let mut header = [0; 5];
        guest::read_words(memory, address, &mut header[..2])?;
        if header[1] != 16 || header[0] != 20 {
            return Ok(INVALID_ARGUMENT);
        }
        guest::read_words(memory, address, &mut header)?;
        if header[3] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if header[2] != 0 {
            return Ok(INVALID_ARGUMENT);
        }
        if acquired {
            return Ok(0x8007_00aa);
        }
        *self = Self {
            requested: header[4],
            capacity: header[4].min(1024),
        };
        Ok(0)
    }
}
