use crate::{abi::media::MediaRecordKind, process::wasm::mutate};
pub(crate) fn pending_version_call() -> u32 {
    mutate(|instance| {
        let wait = instance.pending_version_call()?;
        super::record(
            instance,
            MediaRecordKind::VersionWait,
            wait.map_or(0, |wait| wait.key),
            wait.map_or([0; 5], |wait| {
                [u64::from(wait.token), u64::from(wait.ready), 0, 0, 0]
            }),
        );
        Ok(())
    })
}
pub(crate) fn resume_version_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_version_call(super::wide(low, high), token))
}
