use super::super::{REGISTRY, status};
use crate::{
    abi::cpu_helper::CPU_PAGE_FAULT_TRANSPORT,
    process::{EngineInstance, HostError, StoreCompletion},
};

// only cpu execution exports use these adapters; configuration and diagnostic
// memory exports keep the generic mutate/store transport contracts.
pub(in crate::process::wasm) fn cpu_mutate(
    operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>,
) -> u32 {
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
        let before = instance.cpu_page_fault_sequence32();
        match operation(instance) {
            Ok(()) if instance.cpu_page_fault_sequence32() != before => CPU_PAGE_FAULT_TRANSPORT,
            Ok(()) => 0,
            Err(error) => status(error),
        }
    })
}
pub(in crate::process::wasm) fn cpu_paging_store(
    operation: impl FnOnce(&mut EngineInstance) -> Result<StoreCompletion, HostError>,
) -> u32 {
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
        let before = instance.cpu_page_fault_sequence32();
        match operation(instance) {
            Ok(StoreCompletion::Complete) if instance.cpu_page_fault_sequence32() != before => {
                CPU_PAGE_FAULT_TRANSPORT
            }
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        }
    })
}
