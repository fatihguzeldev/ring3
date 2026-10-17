mod calls;
mod context;
mod marshaling;
mod output;
pub(in crate::process::windows) mod publication;

pub(in crate::process) use context::ComdlgRuntime32;
pub(in crate::process::windows) use marshaling::CapturedFileDialog32;

impl crate::process::EngineInstance {
    pub fn register_commdlg32_module(
        &mut self,
        base: u32,
    ) -> Result<(), crate::process::HostError> {
        self.register_system_module(crate::windows::commdlg::commdlg32_module(base))
    }
}

#[cfg(test)]
pub(in crate::process::windows) mod tests;

impl From<crate::windows::commdlg::ComdlgError32> for crate::process::HostError {
    fn from(error: crate::windows::commdlg::ComdlgError32) -> Self { Self::Comdlg(error) }
}
