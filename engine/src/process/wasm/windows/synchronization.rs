use super::{record, wide};
use crate::{
    abi::media::MediaRecordKind,
    process::{HostError, wasm::mutate},
    windows::synchronization::SynchronizationLimits32,
};

pub(crate) fn configure_synchronization(
    events: u32,
    handles: u32,
    bindings: u32,
    waits: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_synchronization(SynchronizationLimits32 {
            events,
            handles,
            bindings,
            waits,
        })?;
        let epoch = instance.synchronization()?.epoch();
        record(
            instance,
            MediaRecordKind::SynchronizationWait,
            epoch,
            [0; 5],
        );
        Ok(())
    })
}

pub(crate) fn pending_windows_wait() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let (epoch, values) = if let Some(wait) = instance.pending_windows_wait() {
            let flags = u64::from(wait.ready)
                | u64::from(wait.deadline_ns.is_some()) << 1
                | u64::from(wait.alertable) << 2
                | u64::from(wait.wake_mask) << 32;
            (
                wait.epoch,
                [
                    u64::from(wait.token),
                    wait.wait_id,
                    wait.deadline_ns.unwrap_or(0),
                    wait.revision,
                    flags,
                ],
            )
        } else {
            (0, [0; 5])
        };
        record(
            instance,
            MediaRecordKind::SynchronizationWait,
            epoch,
            values,
        );
        Ok(())
    })
}

pub(crate) fn resume_windows_wait(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_windows_wait(wide(low, high), token))
}

pub(crate) fn cancel_windows_wait(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.cancel_windows_wait(wide(low, high), token))
}

pub(crate) fn signal_windows_event(epoch_low: u32, epoch_high: u32, handle: u32) -> u32 {
    mutate(|instance| {
        if handle == 0 {
            return Err(HostError::InvalidRequest);
        }
        instance.signal_windows_event(wide(epoch_low, epoch_high), handle)
    })
}
