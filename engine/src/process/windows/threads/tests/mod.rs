mod context;
mod fls;
mod ownership;
mod scheduling;
mod static_tls;
mod timers;
mod tls;
mod utilities;

mod completion;

mod automation;

impl crate::process::EngineInstance {
    pub(in crate::process::windows) fn create_test_thread_at_gate(
        &mut self,
        entry: u32,
        return_pc: u32,
    ) -> Result<u32, crate::process::HostError> {
        self.create_windows_thread_at_gate(&[0, 0, entry, 0, 0, 0], return_pc)
    }
}
