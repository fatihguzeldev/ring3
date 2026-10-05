use super::{CallError, EngineInstance, HostError};
use crate::{
    abi::arena::TRANSFER_SIZE,
    loader::{
        LinkedImageMetadata32V2, LinkedImageMetadata32V3, LinkedImageMetadata32V4,
        LinkedImageMetadata32V5,
    },
};

const MAX_INPUT_BYTES: u32 = 16 * 1024 * 1024;

pub(super) struct ImageInput {
    total: u32,
    bytes: Vec<u8>,
}

impl EngineInstance {
    pub fn begin_image_input(&mut self, total: u32) -> Result<(), HostError> {
        self.image_input_capacity()?;
        if self.image_input.is_some() || total == 0 || total > MAX_INPUT_BYTES {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total as usize)
            .map_err(|_| HostError::Infrastructure)?;
        self.image_input = Some(ImageInput { total, bytes });
        Ok(())
    }

    pub fn append_image_input(&mut self, offset: u32, bytes: &[u8]) -> Result<(), HostError> {
        self.validate_image_input_append(offset, bytes.len())?;
        let input = self.image_input.as_mut().ok_or(HostError::InvalidRequest)?;
        input.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn abort_image_input(&mut self) -> Result<(), HostError> {
        self.memory()?;
        self.image_input = None;
        Ok(())
    }

    pub fn load_pe32_linked_v2_input_at(
        &mut self,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V2, HostError> {
        let pages = self.image_input_capacity()?;
        let input = self.image_input.as_ref().ok_or(HostError::InvalidRequest)?;
        if input.bytes.len() != input.total as usize {
            return Err(HostError::InvalidRequest);
        }
        let image =
            crate::loader::load_pe32_linked_v2_at(&input.bytes, actual_base, gate_base, pages)
                .map_err(HostError::Loader)?;
        let linked = self.publish_linked_image_v2(image);
        self.image_input = None;
        Ok(linked)
    }

    pub fn load_pe32_linked_v3_input_at(
        &mut self,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V3, HostError> {
        let pages = self.image_input_capacity()?;
        let input = self.image_input.as_ref().ok_or(HostError::InvalidRequest)?;
        if input.bytes.len() != input.total as usize {
            return Err(HostError::InvalidRequest);
        }
        let loaded =
            crate::loader::load_pe32_linked_v3_at(&input.bytes, actual_base, gate_base, pages)
                .map_err(HostError::Loader)?;
        let linked = self.publish_linked_image_v3(loaded);
        self.image_input = None;
        Ok(linked)
    }

    pub fn load_pe32_linked_v4_input_at(
        &mut self,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V4, HostError> {
        let pages = self.image_input_capacity()?;
        let input = self.image_input.as_ref().ok_or(HostError::InvalidRequest)?;
        if input.bytes.len() != input.total as usize {
            return Err(HostError::InvalidRequest);
        }
        let loaded =
            crate::loader::load_pe32_linked_v4_at(&input.bytes, actual_base, gate_base, pages)
                .map_err(HostError::Loader)?;
        let linked = self.publish_linked_image_v4(loaded);
        self.image_input = None;
        Ok(linked)
    }

    pub fn load_pe32_linked_v5_input_at(
        &mut self,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V5, HostError> {
        let pages = self.image_input_capacity()?;
        let input = self.image_input.as_ref().ok_or(HostError::InvalidRequest)?;
        if input.bytes.len() != input.total as usize {
            return Err(HostError::InvalidRequest);
        }
        let loaded =
            crate::loader::load_pe32_linked_v5_at(&input.bytes, actual_base, gate_base, pages)
                .map_err(HostError::Loader)?;
        let linked = self.publish_linked_image_v5(loaded);
        self.image_input = None;
        Ok(linked)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn append_image_input_transfer(
        &mut self,
        offset: u32,
        length: u32,
    ) -> Result<(), HostError> {
        use crate::abi::arena::TRANSFER_OFFSET;

        let length = length as usize;
        self.validate_image_input_append(offset, length)?;
        let input = self.image_input.as_mut().ok_or(HostError::InvalidRequest)?;
        let bytes = &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + length];
        input.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn image_input_capacity(&self) -> Result<u32, HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.image_capacity()
    }

    fn validate_image_input_append(&self, offset: u32, length: usize) -> Result<(), HostError> {
        self.image_input_capacity()?;
        let input = self.image_input.as_ref().ok_or(HostError::InvalidRequest)?;
        let end = input.bytes.len().checked_add(length);
        if length == 0
            || length > TRANSFER_SIZE
            || offset as usize != input.bytes.len()
            || end.is_none_or(|end| end > input.total as usize)
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "image_input_tests.rs"]
mod tests;
