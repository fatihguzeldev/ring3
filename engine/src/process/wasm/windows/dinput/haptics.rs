use super::super::{record, wide};
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        media::{
            MEDIA_RECORD_SIZE, MediaRecordKind,
            haptics::{HAPTIC_FRAME_SIZE, encode_haptic_frame},
        },
    },
    input::dinput::haptics::{HapticCapabilities32, HapticFrame32},
    process::{EngineInstance, HostError, wasm::mutate},
};

fn slot(value: u32) -> Result<u8, HostError> {
    u8::try_from(value).map_err(|_| HostError::InvalidRequest)
}

fn publish(instance: &mut EngineInstance, slot: u8, frame: Option<HapticFrame32>) {
    let length = if frame.is_some() {
        HAPTIC_FRAME_SIZE as u64
    } else {
        0
    };
    let (epoch, sequence, timestamp) = frame
        .map_or((instance.media_epoch().unwrap_or(0), 0, 0), |frame| {
            (frame.epoch, frame.sequence, frame.timestamp_ns)
        });
    record(
        instance,
        MediaRecordKind::DInputHaptics,
        epoch,
        [sequence, u64::from(slot), timestamp, length, 0],
    );
    let start = TRANSFER_OFFSET + MEDIA_RECORD_SIZE;
    let bytes: &mut [u8; HAPTIC_FRAME_SIZE] = (&mut instance.arena.as_mut().get_mut()
        [start..start + HAPTIC_FRAME_SIZE])
        .try_into()
        .unwrap();
    if let Some(frame) = frame {
        encode_haptic_frame(&frame, bytes);
    } else {
        bytes.fill(0);
    }
}

pub(crate) fn configure_controller_haptics(
    epoch_low: u32,
    epoch_high: u32,
    controller: u32,
    axes: u32,
    max_effects: u32,
) -> u32 {
    mutate(|instance| {
        let caps = HapticCapabilities32 {
            axes: u8::try_from(axes).map_err(|_| HostError::InvalidRequest)?,
            max_effects: u16::try_from(max_effects).map_err(|_| HostError::InvalidRequest)?,
        };
        instance.configure_controller_haptics(wide(epoch_low, epoch_high), slot(controller)?, caps)
    })
}

pub(crate) fn render_dinput_haptics(
    epoch_low: u32,
    epoch_high: u32,
    controller: u32,
    time_low: u32,
    time_high: u32,
) -> u32 {
    mutate(|instance| {
        let controller = slot(controller)?;
        let frame = instance.render_dinput_haptics(
            wide(epoch_low, epoch_high),
            controller,
            wide(time_low, time_high),
        )?;
        publish(instance, controller, Some(frame));
        Ok(())
    })
}

pub(crate) fn pending_dinput_haptics(controller: u32) -> u32 {
    mutate(|instance| {
        let controller = slot(controller)?;
        let frame = instance.pending_dinput_haptics(controller)?;
        publish(instance, controller, frame);
        Ok(())
    })
}

pub(crate) fn acknowledge_dinput_haptics(
    epoch_low: u32,
    epoch_high: u32,
    controller: u32,
    sequence_low: u32,
    sequence_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.acknowledge_dinput_haptics(
            wide(epoch_low, epoch_high),
            slot(controller)?,
            wide(sequence_low, sequence_high),
        )
    })
}
