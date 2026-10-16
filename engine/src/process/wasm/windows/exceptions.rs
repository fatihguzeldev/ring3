use crate::{
    abi::{arena::TRANSFER_OFFSET, header},
    process::{EngineInstance, ExceptionDispatch32, wasm::mutate},
};
fn packet(instance: &mut EngineInstance, value: Option<ExceptionDispatch32>) {
    let out = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 64];
    header::write(out, *b"R3EH");
    if let Some(value) = value {
        header::write_u32(out, 12, 1);
        out[16..24].copy_from_slice(&value.id.to_le_bytes());
        for (at, value) in [
            (24, value.thread),
            (28, value.phase as u32),
            (32, value.procedure),
            (36, value.record),
            (40, value.context),
            (44, value.return_pc),
            (48, value.return_id),
            (52, value.code),
        ] {
            header::write_u32(out, at, value);
        }
    }
}
pub(crate) fn dispatch_windows_exception(kl: u32, kh: u32, el: u32, eh: u32) -> u32 {
    mutate(|instance| {
        let value =
            instance.dispatch_windows_exception(super::wide(kl, kh), super::wide(el, eh))?;
        packet(instance, Some(value));
        Ok(())
    })
}
pub(crate) fn dispatch_resident_windows_exception(
    kl: u32,
    kh: u32,
    ul: u32,
    uh: u32,
    el: u32,
    eh: u32,
) -> u32 {
    mutate(|instance| {
        let value = instance.dispatch_resident_windows_exception(
            super::wide(kl, kh),
            super::wide(ul, uh),
            super::wide(el, eh),
        )?;
        packet(instance, Some(value));
        Ok(())
    })
}
pub(crate) fn pending_windows_exception() -> u32 {
    mutate(|instance| {
        let value = instance.pending_windows_exception();
        packet(instance, value);
        Ok(())
    })
}
pub(crate) fn finish_windows_exception_handler(kl: u32, kh: u32, il: u32, ih: u32) -> u32 {
    mutate(|instance| {
        let value =
            instance.finish_windows_exception_handler(super::wide(kl, kh), super::wide(il, ih))?;
        packet(instance, value);
        Ok(())
    })
}
pub(crate) fn register_ntdll_module(base: u32) -> u32 {
    mutate(|instance| instance.register_ntdll_module(base))
}

pub(crate) fn terminate_windows_exception(kl: u32, kh: u32, il: u32, ih: u32) -> u32 {
    mutate(|instance| {
        instance.terminate_windows_exception(super::wide(kl, kh), super::wide(il, ih))
    })
}
