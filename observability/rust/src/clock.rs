#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Clock {
    epoch_ns: u64,
    started: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl Clock {
    pub(crate) fn new() -> Self {
        Self {
            epoch_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| {
                    duration.as_nanos().min(u128::from(u64::MAX)) as u64
                }),
            started: std::time::Instant::now(),
        }
    }
    pub(crate) fn now_ns(&self) -> u64 {
        self.epoch_ns
            .saturating_add(self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct Clock;

#[cfg(target_arch = "wasm32")]
impl Clock {
    pub(crate) fn new() -> Self {
        Self
    }
    pub(crate) fn now_ns(&self) -> u64 {
        host::now()
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod host {
    #[link(wasm_import_module = "ring3_observability")]
    unsafe extern "C" {
        fn now_ns() -> u64;
    }
    pub(super) fn now() -> u64 {
        // the host supplies epoch-anchored monotonic nanoseconds; no memory is shared through this import.
        unsafe { now_ns() }
    }
}
