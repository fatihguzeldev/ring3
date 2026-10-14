use super::{REGISTRY, status};
use crate::cpu::x86::DetectedEntryResult32;
use crate::{
    abi::arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET},
    process::{
        CallError, HostError, calls::call::PendingOwner,
        cpu::interrupts::detection::site::generated_site,
    },
};

fn guard_detection(key: u64, material: u64, resident: bool, pointers: [u32; 3]) -> i64 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return -9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return -5;
        };
        let result = (|| {
            if resident {
                instance
                    .guard_material_resident(key, material)
                    .map_err(status)?;
            } else {
                let generation =
                    u32::try_from(material).map_err(|_| status(HostError::InvalidArtifact))?;
                instance.guard_material(key, generation).map_err(status)?;
            }
            let base = instance.arena_address() as u32;
            if pointers
                != [
                    base + STATE_OFFSET as u32,
                    base + EXIT_OFFSET as u32,
                    base + CANCEL_OFFSET as u32,
                ]
            {
                return Err(1);
            }
            let owner = if resident {
                if let Some(callback) = instance.callback.as_ref() {
                    let record = callback
                        .authorized_resident_record()
                        .ok_or(status(HostError::Call(CallError::Busy)))?;
                    PendingOwner::ResidentCallback {
                        unit_id: material,
                        callback_token: record.token,
                    }
                } else {
                    PendingOwner::Resident(material)
                }
            } else {
                PendingOwner::Replacement(material as u32)
            };
            instance.begin_exception_detection(owner).map_err(status)
        })();
        result.map_or_else(|error| -i64::from(error), |epoch| epoch as i64)
    })
}

pub(crate) fn guard_detect(
    low: u32,
    high: u32,
    generation: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> i64 {
    guard_detection(
        u64::from(low) | (u64::from(high) << 32),
        u64::from(generation),
        false,
        [state, exit, cancel],
    )
}

pub(crate) fn guard_detect_resident(
    low: u32,
    high: u32,
    id_low: u32,
    id_high: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> i64 {
    guard_detection(
        u64::from(low) | (u64::from(high) << 32),
        u64::from(id_low) | (u64::from(id_high) << 32),
        true,
        [state, exit, cancel],
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_exception_detection(
    epoch: i64,
    reason: u32,
    detail: u32,
    address: u32,
    access: u32,
    length: u32,
    debug_origin: u32,
    restart_pc: u32,
    restart_retired: u32,
    restart_flags: u32,
    restart_shadow: u32,
    restart_work: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let result = (|| {
            let epoch = u64::try_from(epoch)
                .ok()
                .filter(|epoch| *epoch != 0)
                .ok_or(HostError::InvalidRequest)?;
            let (event, restart) = generated_site([
                reason,
                detail,
                address,
                access,
                length,
                debug_origin,
                restart_pc,
                restart_retired,
                restart_flags,
                restart_shadow,
                restart_work,
            ])?;
            instance.finish_exception_detection(epoch, event, restart)?;
            instance.deliver_detected_exception(epoch)
        })();
        match result {
            Ok(DetectedEntryResult32::Entered { entered, .. }) if entered.code_invalidated => 11,
            Ok(_) => 0,
            Err(error) => status(error),
        }
    })
}
