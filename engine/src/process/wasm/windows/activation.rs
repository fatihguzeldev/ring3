use crate::{
    abi::media::MediaRecordKind,
    memory::{GuestAddress, PageRange},
    process::{ActivationConfig32, HostError, wasm::mutate},
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn configure_activation(
    contexts: u32,
    frames: u32,
    threads: u32,
    manifest_bytes: u32,
    nodes: u32,
    assemblies: u32,
    section_bytes: u32,
    base: u32,
    pages: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_activation(ActivationConfig32 {
            contexts,
            frames,
            threads,
            manifest_bytes,
            nodes,
            assemblies,
            section_bytes,
            section_bounds: PageRange::new(GuestAddress(base), pages).map_err(HostError::Memory)?,
        })
    })
}
pub(crate) fn pending_activation_call() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let pending = instance.pending_activation_call();
        super::record(
            instance,
            MediaRecordKind::ActivationWait,
            pending.map_or(0, |wait| wait.epoch),
            pending.map_or([0; 5], |wait| {
                [u64::from(wait.token), u64::from(wait.ready), 0, 0, 0]
            }),
        );
        Ok(())
    })
}
pub(crate) fn resume_activation_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_activation_call(super::wide(low, high), token))
}
