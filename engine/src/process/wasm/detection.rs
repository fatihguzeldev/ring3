use super::{REGISTRY, status};
use crate::{
    abi::arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET},
    process::{CallError, HostError, calls::call::PendingOwner},
};

fn guard_detection(key: u64, material: u64, resident: bool, pointers: [u32; 3]) -> i64 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return -9;
        };
        if registry.core_active() {
            return -12;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return -5;
        };
        let result = (|| {
            if resident {
                instance.guard_resident(key, material).map_err(status)?;
            } else {
                let generation =
                    u32::try_from(material).map_err(|_| status(HostError::InvalidArtifact))?;
                instance.guard(key, generation).map_err(status)?;
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

pub(crate) fn take_cpu_event() -> u32 {
    super::mutate(|instance| {
        let event = instance
            .cpu_event
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        if !event.matches_context(instance.arena()) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let mut bytes = [0; crate::abi::x86::CPU_EVENT_SITE_SIZE];
        crate::abi::x86::encode_cpu_event_site32(event, &mut bytes)
            .map_err(|_| HostError::Infrastructure)?;
        let output = crate::abi::arena::TRANSFER_OFFSET;
        instance.arena_mut()?[output..output + bytes.len()].copy_from_slice(&bytes);
        instance.take_cpu_event32()?;
        Ok(())
    })
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
        if registry.core_active() {
            return 12;
        }
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let result = (|| {
            let epoch = u64::try_from(epoch)
                .ok()
                .filter(|epoch| *epoch != 0)
                .ok_or(HostError::InvalidRequest)?;
            instance.finish_generated_detection(
                epoch,
                [
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
                ],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => 0,
            Err(error) => status(error),
        }
    })
}
