use super::mutate;
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    process::{CallError, HostError},
};
use ring3_observability::{CategorySelection, Config, Level, Observer};

pub(crate) fn configure_observability(
    level: u32,
    max_bytes: u32,
    sequence: u32,
    track_low: u32,
    track_high: u32,
    context_length: u32,
) -> u32 {
    mutate(|instance| {
        if instance.observer.is_recording() || instance.observer.stats().buffered_bytes != 0 {
            return Err(HostError::Call(CallError::Busy));
        }
        if context_length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let input = &instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + context_length as usize];
        let selection = std::str::from_utf8(input).map_err(|_| HostError::InvalidRequest)?;
        let categories =
            CategorySelection::from_wire(selection).map_err(|_| HostError::InvalidRequest)?;
        let observer = Observer::new(Config {
            level: Level::try_from(level).map_err(|_| HostError::InvalidRequest)?,
            categories,
            max_buffer_bytes: max_bytes as usize,
            producer_sequence_id: sequence,
            track_uuid: u64::from(track_low) | (u64::from(track_high) << 32),
            ..Config::default()
        })
        .map_err(|_| HostError::InvalidRequest)?;
        instance.set_observer(observer)
    })
}

pub(crate) fn observability_drain() -> u32 {
    drain(false)
}
pub(crate) fn observability_finish() -> u32 {
    drain(true)
}
fn drain(finish: bool) -> u32 {
    mutate(|instance| {
        if finish {
            instance.observer.finalize();
        }
        let bytes = instance.observer.drain_chunk(TRANSFER_SIZE - 16);
        let stats = instance.observer.stats();
        let output = &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
        output.fill(0);
        output[..4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        output[4..8].copy_from_slice(&u32::from(stats.drained()).to_le_bytes());
        output[8..16].copy_from_slice(&stats.dropped_events.to_le_bytes());
        output[16..16 + bytes.len()].copy_from_slice(&bytes);
        Ok(())
    })
}

pub(crate) fn observability_status() -> u32 {
    mutate(|instance| {
        let stats = instance.observer.stats();
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 40];
        output[..4].copy_from_slice(&(stats.level as u32).to_le_bytes());
        output[4..8].copy_from_slice(&u32::from(stats.finished).to_le_bytes());
        output[8..16].copy_from_slice(&stats.dropped_events.to_le_bytes());
        output[16..24].copy_from_slice(&(stats.buffered_bytes as u64).to_le_bytes());
        output[24..32].copy_from_slice(&stats.emitted_events.to_le_bytes());
        output[32..40].copy_from_slice(&stats.open_scopes.to_le_bytes());
        Ok(())
    })
}
