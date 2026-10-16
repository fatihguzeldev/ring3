use super::{record, wide};
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        callback::{CALLBACK_RECORD_SIZE, encode_callback},
        media::MediaRecordKind,
        resident_callback::{
            RESIDENT_CALLBACK_RECORD_SIZE, RESIDENT_CALLBACK_RESULT_SIZE, encode_resident_callback,
            encode_resident_callback_result,
        },
        windows::{WINDOWS_CALLBACK_RECORD_SIZE, WindowsCallbackRecord32, encode_windows_callback},
    },
    process::{HostError, WindowsCallbackProvider32, wasm::mutate},
    windows::CallingConvention32,
};

pub(crate) fn pending_windows_callback() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let request = instance
            .pending_windows_callback()
            .map(|request| WindowsCallbackRecord32 {
                provider: match request.provider {
                    WindowsCallbackProvider32::User32 => 1,
                    WindowsCallbackProvider32::D3D8 => 2,
                    WindowsCallbackProvider32::DirectSound => 3,
                    WindowsCallbackProvider32::DirectInput => 4,
                    WindowsCallbackProvider32::Files => 5,
                    WindowsCallbackProvider32::Resources => 6,
                    WindowsCallbackProvider32::Nls => 7,
                    WindowsCallbackProvider32::Ole32 => 8,
                },
                procedure: request.procedure.0,
                count: request.count,
                continuation: request.continuation,
                arguments: request.arguments,
            });
        encode_windows_callback(
            request,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + WINDOWS_CALLBACK_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
pub(crate) fn windows_gate_info(id: u32) -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let gate = crate::process::windows_gate_info(id).ok_or(HostError::InvalidRequest)?;
        let convention = match gate.convention {
            CallingConvention32::Cdecl => 1,
            CallingConvention32::Stdcall => 2,
            CallingConvention32::Thiscall => 3,
        };
        record(
            instance,
            MediaRecordKind::WindowsGate,
            0,
            [u64::from(id), convention, u64::from(gate.stack_words), 0, 0],
        );
        Ok(())
    })
}
pub(crate) fn capture_windows_call(low: u32, high: u32, generation: u32) -> u32 {
    mutate(|instance| {
        instance
            .capture_windows_call(wide(low, high), generation)
            .map(|_| ())
    })
}
pub(crate) fn capture_resident_windows_call(low: u32, high: u32, id_low: u32, id_high: u32) -> u32 {
    mutate(|instance| {
        instance
            .capture_resident_windows_call(wide(low, high), wide(id_low, id_high))
            .map(|_| ())
    })
}
pub(crate) fn capture_active_resident_callback_windows_call(
    low: u32,
    high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
) -> u32 {
    mutate(|instance| {
        instance
            .capture_active_resident_callback_windows_call(
                wide(low, high),
                wide(id_low, id_high),
                token,
            )
            .map(|_| ())
    })
}
pub(crate) fn begin_windows_resident_callback(
    low: u32,
    high: u32,
    id_low: u32,
    id_high: u32,
    return_pc: u32,
    return_id: u32,
) -> u32 {
    mutate(|instance| {
        let result = instance.begin_windows_resident_callback(
            wide(low, high),
            wide(id_low, id_high),
            return_pc,
            return_id,
        )?;
        encode_resident_callback(
            &result,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + RESIDENT_CALLBACK_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
pub(crate) fn finish_windows_resident_callback(
    low: u32,
    high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
) -> u32 {
    mutate(|instance| {
        let result = instance.finish_windows_resident_callback(
            wide(low, high),
            wide(id_low, id_high),
            token,
        )?;
        encode_resident_callback_result(
            &result,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + RESIDENT_CALLBACK_RESULT_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
pub(crate) fn begin_windows_callback(
    low: u32,
    high: u32,
    generation: u32,
    return_pc: u32,
    return_id: u32,
) -> u32 {
    mutate(|instance| {
        let result =
            instance.begin_windows_callback(wide(low, high), generation, return_pc, return_id)?;
        encode_callback(
            &result,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
pub(crate) fn finish_windows_callback(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    mutate(|instance| {
        let result = instance.finish_windows_callback(wide(low, high), generation, token)?;
        encode_callback(
            &result,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}
