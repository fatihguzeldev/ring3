use super::super::mutate;
use crate::{
    abi::{arena::TRANSFER_OFFSET, execution::receipt::*},
    process::HostError,
};

pub(crate) fn begin_execution_receipt(
    key_low: u32,
    key_high: u32,
    budget: u32,
    unit_low: u32,
    unit_high: u32,
    dispatch: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let unit = u64::from(unit_low) | (u64::from(unit_high) << 32);
    mutate(|instance| {
        let dispatch = match dispatch {
            0 => false,
            1 => true,
            _ => return Err(HostError::InvalidRequest),
        };
        let frame = instance.begin_execution_receipt(key, budget, unit, dispatch)?;
        let output = &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + EXECUTION_FRAME_SIZE];
        encode_execution_frame(frame, output.try_into().unwrap());
        Ok(())
    })
}

pub(crate) fn finish_execution_receipt(
    key_low: u32,
    key_high: u32,
    token_low: u32,
    token_high: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let token = u64::from(token_low) | (u64::from(token_high) << 32);
    mutate(|instance| {
        let receipt = instance.finish_execution_receipt(key, token)?;
        let output = &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + EXECUTION_RECEIPT_SIZE];
        encode_execution_receipt(receipt, output.try_into().unwrap());
        Ok(())
    })
}
