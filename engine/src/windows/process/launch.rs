use super::ProcessError32;

/// Explicit virtual startup data. No host environment, identity, or wall clock leaks.
#[derive(Clone, Debug)]
pub struct ProcessLaunch32 {
    pub process_id: u32,
    pub image_path: String,
    pub command_line: String,
    pub current_directory: String,
    pub environment: Vec<(String, String)>,
    /// UTC FILETIME at the configured monotonic clock's epoch, in 100ns units.
    pub wall_clock_filetime: u64,
    pub environment_units: usize,
    pub startup_bytes: usize,
}
impl Default for ProcessLaunch32 {
    fn default() -> Self {
        Self {
            process_id: 1,
            image_path: "C:\\game\\main.exe".into(),
            command_line: "main.exe".into(),
            current_directory: "C:\\game".into(),
            environment: Vec::new(),
            wall_clock_filetime: 116_444_736_000_000_000,
            environment_units: 32768,
            startup_bytes: 65536,
        }
    }
}
impl ProcessLaunch32 {
    pub fn validate(&self) -> Result<(), ProcessError32> {
        if self.process_id == 0
            || !(4096..=1024 * 1024).contains(&self.startup_bytes)
            || !self.startup_bytes.is_multiple_of(4096)
            || [
                &self.image_path,
                &self.command_line,
                &self.current_directory,
            ]
            .iter()
            .any(|value| value.contains('\0') || value.encode_utf16().count() > 32766)
        {
            return Err(ProcessError32::InvalidParameter);
        }
        Ok(())
    }
}
