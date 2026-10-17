use crate::{
    memory::PageRange,
    windows::process::{Environment32, ProcessError32, ProcessLaunch32},
};

pub(in crate::process) struct WindowsProcessRuntime32 {
    pub(super) configured: bool,
    pub(super) launch_input: Option<super::input::LaunchInput32>,
    pub(super) launch: ProcessLaunch32,
    pub(super) environment: Environment32,
    pub(in crate::process) startup: Option<super::startup::ProcessStartup32>,
    pub(super) standard_handles: [u32; 3],
    pub(super) startup_flags: u32,
    pub(super) priority_class: u32,
    pub(super) atoms: super::atoms::AtomTable32,
}
impl WindowsProcessRuntime32 {
    pub(in crate::process) fn new(launch: ProcessLaunch32) -> Result<Self, ProcessError32> {
        launch.validate()?;
        let mut environment = Environment32::new(launch.environment_units)?;
        for (name, value) in &launch.environment {
            environment.set(name, Some(value))?;
        }
        Ok(Self {
            configured: false,
            launch_input: None,
            launch,
            environment,
            startup: None,
            standard_handles: [0; 3],
            startup_flags: 0,
            priority_class: 0x20,
            atoms: Default::default(),
        })
    }
    pub(in crate::process) fn reservation(&self) -> Option<PageRange> {
        self.startup.as_ref().map(|startup| startup.range)
    }
}
impl crate::process::EngineInstance {
    pub(super) fn process_standard_handles(&self) -> Result<[u32; 3], crate::process::HostError> {
        let Some(startup) = self.process_runtime.startup.as_ref() else {
            return Ok(self.process_runtime.standard_handles);
        };
        let mut bytes = [0; 12];
        self.memory()?
            .read(crate::memory::GuestAddress(startup.peb + 0x118), &mut bytes)
            .map_err(|_| crate::process::HostError::WindowsProcess(ProcessError32::NoAccess))?;
        Ok(core::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        }))
    }
    pub fn configure_process_standard_handles(
        &mut self,
        handles: [u32; 3],
    ) -> Result<(), crate::process::HostError> {
        self.memory()?;
        if self.image_started
            || self.process_runtime.startup.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(crate::process::HostError::InvalidRequest);
        }
        self.process_runtime.standard_handles = handles;
        self.process_runtime.startup_flags |= 0x100;
        Ok(())
    }
    pub(in crate::process) fn current_windows_process_id(&self) -> u32 {
        self.process_runtime.launch.process_id
    }
    pub(in crate::process) fn process_creation_filetime(
        &self,
    ) -> Result<u64, crate::process::HostError> {
        if self.cpu_clock.monotonic_bounds().is_ok() {
            self.windows_filetime()
        } else {
            Ok(self.process_runtime.launch.wall_clock_filetime)
        }
    }
    pub fn configure_process_launch(
        &mut self,
        launch: ProcessLaunch32,
    ) -> Result<(), crate::process::HostError> {
        use crate::process::HostError;
        self.memory()?;
        if self.image_started
            || self.process_runtime.startup.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        let mut candidate =
            WindowsProcessRuntime32::new(launch).map_err(HostError::WindowsProcess)?;
        candidate.standard_handles = self.process_runtime.standard_handles;
        candidate.startup_flags = self.process_runtime.startup_flags;
        self.apply_launch_file_directory(&candidate.launch.current_directory)?;
        candidate.configured = true;
        self.process_runtime = candidate;
        Ok(())
    }
    pub(in crate::process) fn process_launch_directory(&self) -> Option<&str> {
        (self.process_runtime.configured || self.process_runtime.startup.is_some())
            .then_some(self.process_runtime.launch.current_directory.as_str())
    }
    pub(in crate::process) fn process_path_environment(&self) -> Option<&str> {
        self.process_runtime.environment.get("PATH")
    }
    pub(in crate::process) fn current_windows_thread_id(
        &self,
    ) -> Result<u32, crate::process::HostError> {
        self.memory()?;
        Ok(self.threads.current_id())
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn windows_filetime(&self) -> Result<u64, crate::process::HostError> {
        let (epoch, now) = self.cpu_clock.monotonic_bounds()?;
        self.process_runtime
            .launch
            .wall_clock_filetime
            .checked_add((now - epoch) / 100)
            .ok_or(crate::process::HostError::InvalidRequest)
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn expand_windows_environment(&self, value: &str) -> String {
        self.process_runtime.environment.expand(value)
    }
}
