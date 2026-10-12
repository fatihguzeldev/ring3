use super::{REGISTRY, mutate, status};
use crate::cpu::dbt::RegistryError;
use crate::process::HostError;

pub(crate) fn select_resident_callback_unit(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    target_low: u32,
    target_high: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let home_id = u64::from(home_low) | (u64::from(home_high) << 32);
        let target_id = u64::from(target_low) | (u64::from(target_high) << 32);
        match instance.select_resident_callback_unit(key, home_id, callback_token, target_id) {
            Ok(()) => 0,
            Err(HostError::Resident(RegistryError::NotFound { .. })) => 17,
            Err(error) => status(error),
        }
    })
}

pub(crate) fn capture_call(
    low: u32,
    high: u32,
    generation: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .capture_call_raw(key, generation, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn capture_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance
            .capture_resident_call_raw(key, id, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn complete_call(low: u32, high: u32, generation: u32, token: u32, result: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.complete_call(key, generation, token, result))
}

pub(crate) fn complete_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
    result: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.complete_resident_call(key, id, token, result))
}

pub(crate) fn abandon_call(low: u32, high: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.abandon_call(key, token))
}

pub(crate) fn complete_windows_call(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.complete_windows_call(key, generation, token))
}

pub(crate) fn complete_resident_windows_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.complete_resident_windows_call(key, id, token))
}

pub(crate) fn capture_active_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance
            .capture_active_resident_callback_call_raw(key, id, callback_token, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn complete_active_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
    result: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance.complete_active_resident_callback_call(
            key,
            id,
            callback_token,
            inner_token,
            result,
        )
    })
}

pub(crate) fn complete_active_resident_callback_windows_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance.complete_active_resident_callback_windows_call(
            key,
            id,
            callback_token,
            inner_token,
        )
    })
}

pub(crate) fn capture_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance
            .capture_resident_callback_call_raw(key, id, callback_token, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn complete_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
    result: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance.complete_resident_callback_call(key, id, callback_token, inner_token, result)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_callback(
    low: u32,
    high: u32,
    generation: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match instance.begin_callback_from_transfer(
            key,
            generation,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        ) {
            Ok(record) => {
                if record.outcome == 1 {
                    17
                } else {
                    0
                }
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn finish_callback(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.finish_callback(key, generation, token).map(|_| ()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_resident_callback(
    key_low: u32,
    key_high: u32,
    outer_low: u32,
    outer_high: u32,
    callback_low: u32,
    callback_high: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let outer_id = u64::from(outer_low) | (u64::from(outer_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match instance.begin_resident_callback_from_transfer(
            key,
            outer_id,
            callback_id,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        ) {
            Ok(record) => {
                if record.outcome == 0 {
                    0
                } else {
                    17
                }
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn finish_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    mutate(|instance| {
        instance
            .finish_resident_callback(key, callback_id, token)
            .map(|_| ())
    })
}

pub(crate) fn authorize_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    mutate(|instance| instance.authorize_resident_callback(key, callback_id, token))
}

pub(crate) fn abort_callback(low: u32, high: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.abort_callback(key, token))
}

pub(crate) fn resume_callback_code(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .resume_callback_code(key, generation, callback_token, count, gate_count)
            .map(|_| ())
    })
}

pub(crate) fn resume_callback_entries(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .resume_callback_entries(key, generation, callback_token, count, gate_count)
            .map(|_| ())
    })
}
