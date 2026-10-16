use crate::{
    memory::{GuestAddress, PageRange},
    process::EngineInstance,
};

impl EngineInstance {
    pub(in crate::process) fn automation_scratch_bounds(&self) -> PageRange {
        PageRange::new(GuestAddress(0x69d0_0000), 32).expect("bounded automation callback arena")
    }
}
