use super::{REGISTRY, status};
use crate::abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE};

pub(crate) fn begin_execution_census(key_low: u32, key_high: u32, capacity: u32) -> u32 {
    control(key_low, key_high, |instance, key| {
        instance.begin_execution_census(key, capacity)
    })
}

pub(crate) fn stop_execution_census(key_low: u32, key_high: u32) -> u32 {
    control(key_low, key_high, |instance, key| {
        instance.stop_execution_census(key)
    })
}

fn control(
    key_low: u32,
    key_high: u32,
    operation: impl FnOnce(
        &mut crate::process::EngineInstance,
        u64,
    ) -> Result<(), crate::process::HostError>,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let key = u64::from(key_low) | u64::from(key_high) << 32;
        operation(instance, key).map_or_else(status, |()| 0)
    })
}

pub(crate) fn execution_census_page(key_low: u32, key_high: u32, offset: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let key = u64::from(key_low) | u64::from(key_high) << 32;
        let page = match instance.execution_census_page(key, offset) {
            Ok(page) => page,
            Err(error) => return status(error),
        };
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]
            .copy_from_slice(&page);
        0
    })
}
