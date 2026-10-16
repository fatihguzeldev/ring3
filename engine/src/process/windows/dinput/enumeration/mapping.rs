use super::PendingEnumeration32;
use crate::{
    input::dinput::DirectInputResult32 as H,
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, Permissions},
};

impl PendingEnumeration32 {
    pub(in crate::process::windows::dinput) fn check_mapping(
        &self,
        memory: &AddressSpace,
    ) -> Result<(), H> {
        if self.memory != memory.identity() {
            return Err(H::INVALID_PARAM);
        }
        for page in self.range.indices() {
            let address = GuestAddress(page as u32 * PAGE_SIZE);
            if memory.mapping_version_at(address)? != self.mapping
                || memory.permissions_at(address)? != Permissions::READ_WRITE
            {
                return Err(H::INVALID_PARAM);
            }
        }
        Ok(())
    }
}
pub(in crate::process::windows::dinput) fn relocated(
    row: &[u8],
    fixups: Option<&[usize]>,
    base: u32,
) -> Result<Vec<u8>, H> {
    let mut bytes = super::super::memory::bytes(row.len())?;
    bytes.copy_from_slice(row);
    for &at in fixups.unwrap_or(&[]) {
        let word = bytes
            .get_mut(at..at.checked_add(4).ok_or(H::INVALID_PARAM)?)
            .ok_or(H::INVALID_PARAM)?;
        let offset = u32::from_le_bytes(word.try_into().unwrap());
        let pointer = base.checked_add(offset).ok_or(H::INVALID_PARAM)?;
        word.copy_from_slice(&pointer.to_le_bytes());
    }
    Ok(bytes)
}
