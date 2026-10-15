use super::{input_bytes, record};
use crate::abi::media::MediaRecordKind;
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    process::{HostError, MediaError, wasm::mutate},
};

pub(crate) fn submit_input(length: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let input = input_bytes(instance, length)?;
        bytes[..input.len()].copy_from_slice(input);
        instance.submit_input_encoded(&bytes[..length as usize])
    })
}

pub(crate) fn input_state() -> u32 {
    mutate(|instance| {
        let input = instance.input()?;
        let state = *input.state();
        let epoch = input.session_epoch();
        let flags = u64::from(state.focused) | u64::from(state.captured) << 1;
        let buttons = state
            .buttons
            .iter()
            .enumerate()
            .fold(0u64, |value, (i, pressed)| {
                value | (u64::from(*pressed) << i)
            });
        record(
            instance,
            MediaRecordKind::InputState,
            epoch,
            [
                flags,
                buttons,
                state.relative.dx as u64,
                state.relative.dy as u64,
                state.relative.wheel as u64,
            ],
        );
        let bytes =
            &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET + 64..TRANSFER_OFFSET + 96];
        bytes.fill(0);
        for (index, pressed) in state.keys.iter().enumerate() {
            if *pressed {
                bytes[index / 8] |= 1 << (index % 8);
            }
        }
        Ok(())
    })
}

pub(crate) fn input_read_queue(max: u32, capacity: u32, consume: u32) -> u32 {
    mutate(|instance| {
        if capacity as usize > TRANSFER_SIZE || consume > 1 {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = [0; TRANSFER_SIZE];
        let output = &mut bytes[..capacity as usize];
        let read = if consume != 0 {
            instance.input_read_encoded(max as usize, output)?
        } else {
            instance
                .input()?
                .peek_encoded(max as usize, output)
                .map_err(|error| HostError::Media(MediaError::Input(error)))?
        };
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + read.bytes]
            .copy_from_slice(&bytes[..read.bytes]);
        Ok(())
    })
}

pub(crate) fn input_consume(count: u32) -> u32 {
    mutate(|instance| instance.input_consume(count as usize))
}

pub(crate) fn input_drain_relative() -> u32 {
    mutate(|instance| {
        let totals = instance.input_drain_relative()?;
        let epoch = instance.media_epoch()?;
        record(
            instance,
            MediaRecordKind::InputRelative,
            epoch,
            [
                totals.dx as u64,
                totals.dy as u64,
                totals.wheel as u64,
                0,
                0,
            ],
        );
        Ok(())
    })
}
