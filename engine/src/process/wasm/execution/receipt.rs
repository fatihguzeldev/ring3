use super::super::{REGISTRY, mutate, mutate_execution_terminal, status};
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
    begin_receipt(key_low, key_high, budget, unit_low, unit_high, dispatch, 0)
}

pub(crate) fn begin_owned_execution_receipt(
    key_low: u32,
    key_high: u32,
    budget: u32,
    unit_low: u32,
    unit_high: u32,
    dispatch: u32,
) -> u32 {
    begin_receipt(key_low, key_high, budget, unit_low, unit_high, dispatch, 1)
}

pub(crate) fn begin_compiled_execution_receipt(
    key_low: u32,
    key_high: u32,
    budget: u32,
    unit_low: u32,
    unit_high: u32,
    dispatch: u32,
) -> u32 {
    begin_receipt(key_low, key_high, budget, unit_low, unit_high, dispatch, 2)
}

pub(crate) fn compiled_execution_usage(key_low: u32, key_high: u32) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.core_active() {
            return 12;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let usage = match instance.compiled_execution_usage(key) {
            Ok(usage) => usage,
            Err(error) => return status(error),
        };
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 80];
        output.fill(0);
        output[..4].copy_from_slice(b"R3EU");
        output[4..6].copy_from_slice(&1u16.to_le_bytes());
        output[6..8].copy_from_slice(&1u16.to_le_bytes());
        output[8..12].copy_from_slice(&80u32.to_le_bytes());
        for (index, count) in [
            usage.seed_full_need_code_seals,
            usage.strict_full_need_code_seals,
            usage.light_need_code_seals,
            usage.full_materializations,
            usage.eligible_children,
            usage.strict_children,
            usage.continued_x87_step_attempts,
            usage.rejected_public_transitions,
        ]
        .into_iter()
        .enumerate()
        {
            output[16 + index * 8..24 + index * 8].copy_from_slice(&count.to_le_bytes());
        }
        0
    })
}

fn begin_receipt(
    key_low: u32,
    key_high: u32,
    budget: u32,
    unit_low: u32,
    unit_high: u32,
    dispatch: u32,
    mode: u8,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let unit = u64::from(unit_low) | (u64::from(unit_high) << 32);
    mutate(|instance| {
        let dispatch = match dispatch {
            0 => false,
            1 => true,
            _ => return Err(HostError::InvalidRequest),
        };
        let frame = match mode {
            0 => instance.begin_execution_receipt(key, budget, unit, dispatch),
            1 => instance.begin_owned_execution_receipt(key, budget, unit, dispatch),
            2 => instance.begin_compiled_execution_receipt(key, budget, unit, dispatch),
            _ => unreachable!("private receipt mode"),
        }?;
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
    mutate_execution_terminal(|instance| {
        let receipt = instance.finish_execution_receipt(key, token)?;
        let output = &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + EXECUTION_RECEIPT_SIZE];
        encode_execution_receipt(receipt, output.try_into().unwrap());
        Ok(())
    })
}
