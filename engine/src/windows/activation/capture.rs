use std::sync::{Arc, Weak};

#[derive(Clone, Default)]
pub(crate) struct CapturedActivation32 {
    handle: u32,
    captured: bool,
    lease: Option<Arc<()>>,
}
impl CapturedActivation32 {
    pub(crate) fn new(handle: u32) -> Self {
        Self {
            handle,
            captured: true,
            lease: (handle != 0).then(|| Arc::new(())),
        }
    }
    pub(crate) fn is_captured(&self) -> bool {
        self.captured
    }
    pub(crate) fn handle(&self) -> u32 {
        self.handle
    }
    pub(crate) fn lease(&self) -> Option<Weak<()>> {
        self.lease.as_ref().map(Arc::downgrade)
    }
}
