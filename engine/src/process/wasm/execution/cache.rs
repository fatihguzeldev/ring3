use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        execution::{
            cache::{
                CACHE_REQUEST_SIZE, CACHE_USAGE_SIZE, encode_cache_request, encode_cache_usage,
            },
            encode_installation,
        },
    },
    process::{EngineInstance, ExecutionConfig32, wasm::mutate},
};
fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}
fn request(instance: &mut EngineInstance, value: Option<crate::process::CacheRequest32>) {
    encode_cache_request(
        value,
        (&mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + CACHE_REQUEST_SIZE])
            .try_into()
            .unwrap(),
    );
}
pub(crate) fn configure_execution(units: u32, wasm_bytes: u32, promotion_hits: u32) -> u32 {
    mutate(|instance| {
        instance.configure_execution(ExecutionConfig32 {
            units,
            wasm_bytes,
            promotion_hits,
        })
    })
}
pub(crate) fn prepare_cache_entry(low: u32, high: u32, pc: u32) -> u32 {
    mutate(|instance| {
        let value = instance.prepare_cache_entry(wide(low, high), pc)?;
        request(instance, Some(value));
        Ok(())
    })
}
pub(crate) fn pending_cache_request() -> u32 {
    mutate(|instance| {
        let value = instance.pending_cache_request()?;
        request(instance, value);
        Ok(())
    })
}
pub(crate) fn prepare_cache_promotion(low: u32, high: u32) -> u32 {
    mutate(|instance| {
        let value = instance.prepare_cache_promotion(wide(low, high))?;
        request(instance, value);
        Ok(())
    })
}
pub(crate) fn acknowledge_cache_eviction(
    low: u32,
    high: u32,
    serial_low: u32,
    serial_high: u32,
    victim_low: u32,
    victim_high: u32,
    slot: u32,
) -> u32 {
    mutate(|instance| {
        instance.acknowledge_cache_eviction(
            wide(low, high),
            wide(serial_low, serial_high),
            wide(victim_low, victim_high),
            slot,
        )
    })
}
pub(crate) fn install_cached_entry(
    low: u32,
    high: u32,
    serial_low: u32,
    serial_high: u32,
    unit_low: u32,
    unit_high: u32,
    slot: u32,
) -> u32 {
    mutate(|instance| {
        let value = instance.install_cached_entry(
            wide(low, high),
            wide(serial_low, serial_high),
            wide(unit_low, unit_high),
            slot,
        )?;
        encode_installation(
            value.unit_id,
            value.slot,
            (&mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32])
                .try_into()
                .unwrap(),
        );
        Ok(())
    })
}
pub(crate) fn cancel_cache_request(low: u32, high: u32, serial_low: u32, serial_high: u32) -> u32 {
    mutate(|instance| instance.cancel_cache_request(wide(low, high), wide(serial_low, serial_high)))
}
pub(crate) fn cache_usage() -> u32 {
    mutate(|instance| {
        let usage = instance.cache_usage()?;
        encode_cache_usage(
            usage,
            (&mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + CACHE_USAGE_SIZE])
                .try_into()
                .unwrap(),
        );
        Ok(())
    })
}
