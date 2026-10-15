use super::super::super::memory::{bytes, put, read, words};
use crate::{
    input::dinput::{DirectInputResult32 as H, actions::Action32},
    memory::{AddressSpace, GuestAddress},
};
pub(in crate::process::windows::dinput) struct ActionMap32 {
    pub header: Vec<u8>,
    pub address: u32,
    pub action_address: u32,
    pub actions: Vec<Action32>,
    pub data_size: u32,
    pub buffer: u32,
    pub minimum: i32,
    pub maximum: i32,
}
pub(in crate::process::windows::dinput) fn read_action_map(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
) -> Result<ActionMap32, H> {
    let [size, action_size, data_size, count, action_address] = words(memory, address)?;
    if size != if wide { 588 } else { 328 }
        || action_size != 40
        || count > 1024
        || data_size != count.checked_mul(4).ok_or(H::INVALID_PARAM)?
        || count != 0 && action_address == 0
    {
        return Err(H::INVALID_PARAM);
    }
    let mut header = bytes(size as usize)?;
    memory.read(GuestAddress(address), &mut header)?;
    let mut actions = Vec::new();
    actions
        .try_reserve_exact(count as usize)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    for index in 0..count {
        let at = action_address
            .checked_add(index * 40)
            .ok_or(H::INVALID_PARAM)?;
        let record = read::<40>(memory, at)?;
        let field = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
        let (app_data, semantic, flags, name) = (field(0), field(4), field(8), field(12));
        let instance = record[16..32].try_into().unwrap();
        let (object_id, how) = (field(32), field(36));
        actions.push(Action32 {
            app_data,
            semantic,
            flags,
            name,
            instance,
            object_id,
            how,
        });
    }
    let value = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let buffer = value(40);
    let minimum = value(44) as i32;
    let maximum = value(48) as i32;
    Ok(ActionMap32 {
        header,
        address,
        action_address,
        actions,
        data_size,
        buffer,
        minimum,
        maximum,
    })
}
impl ActionMap32 {
    pub fn encoded_actions(&self) -> Result<Vec<u8>, H> {
        let mut bytes = bytes(self.actions.len() * 40)?;
        for (index, action) in self.actions.iter().enumerate() {
            let at = index * 40;
            for (offset, value) in [
                (0, action.app_data),
                (4, action.semantic),
                (8, action.flags),
                (12, action.name),
                (32, action.object_id),
                (36, action.how),
            ] {
                put(&mut bytes, at + offset, value);
            }
            bytes[at + 16..at + 32].copy_from_slice(&action.instance);
        }
        Ok(bytes)
    }
}
