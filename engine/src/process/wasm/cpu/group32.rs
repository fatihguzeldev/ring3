use super::{REGISTRY, StoreCompletion, status};
use crate::cpu::dbt::X87WideGroupProgress32;

#[allow(clippy::too_many_arguments)]
pub(crate) fn x87_resident_group32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    ordinal: u32,
    address0: u32,
    address1: u32,
    address2: u32,
    address3: u32,
    address4: u32,
    address5: u32,
    address6: u32,
    address7: u32,
    address8: u32,
    address9: u32,
    address10: u32,
    address11: u32,
    address12: u32,
    address13: u32,
    address14: u32,
    address15: u32,
    address16: u32,
    address17: u32,
    address18: u32,
    address19: u32,
    address20: u32,
    address21: u32,
    address22: u32,
    address23: u32,
    address24: u32,
    address25: u32,
    address26: u32,
    address27: u32,
    address28: u32,
    address29: u32,
    address30: u32,
    address31: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return X87WideGroupProgress32::encode(9, 0, 0) as i64;
        };
        if registry.core_active() {
            return X87WideGroupProgress32::encode(12, 0, 0) as i64;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return X87WideGroupProgress32::encode(5, 0, 0) as i64;
        };
        let before = instance.cpu_page_fault_sequence32();
        let outcome = instance.x87_resident_group32(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            ordinal,
            [
                address0, address1, address2, address3, address4, address5, address6, address7,
                address8, address9, address10, address11, address12, address13, address14,
                address15, address16, address17, address18, address19, address20, address21,
                address22, address23, address24, address25, address26, address27, address28,
                address29, address30, address31,
            ],
            flags,
            max_steps,
        );
        let transport = match outcome.result {
            Ok(StoreCompletion::Complete) if instance.cpu_page_fault_sequence32() != before => {
                crate::abi::cpu_helper::CPU_PAGE_FAULT_TRANSPORT
            }
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        };
        X87WideGroupProgress32::encode(transport, outcome.completed, outcome.reported_step) as i64
    })
}
