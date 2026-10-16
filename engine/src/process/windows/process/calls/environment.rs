use super::*;

impl EngineInstance {
    pub(super) fn execute_process_environment_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let wide = api.name().ends_with('W');
        let code_page = self.windows_ansi_code_page();
        let result = match api {
            GetCommandLineA | GetCommandLineW => {
                self.process_runtime
                    .startup
                    .as_ref()
                    .ok_or(guest(ProcessError32::InvalidParameter))?
                    .command_line[usize::from(wide)]
            }
            GetEnvironmentVariableA | GetEnvironmentVariableW => {
                let name =
                    buffers::string(self.memory()?, args[0], wide, code_page).map_err(guest)?;
                let value = self
                    .process_runtime
                    .environment
                    .get(&name)
                    .ok_or(guest(ProcessError32::NotFound))?
                    .to_owned();
                let count = buffers::counted(
                    self.memory.as_mut().unwrap(),
                    args[1],
                    args[2],
                    &value,
                    wide,
                    code_page,
                )
                .map_err(guest)?;
                if value.is_empty() {
                    return Ok(ProcessReply32 {
                        value: 0,
                        error: Some(0),
                    });
                }
                count
            }
            SetEnvironmentVariableA | SetEnvironmentVariableW => {
                let name =
                    buffers::string(self.memory()?, args[0], wide, code_page).map_err(guest)?;
                let value = if args[1] == 0 {
                    None
                } else {
                    Some(buffers::string(self.memory()?, args[1], wide, code_page).map_err(guest)?)
                };
                let mut environment = self.process_runtime.environment.clone();
                environment.set(&name, value.as_deref()).map_err(guest)?;
                if let Some(startup) = self.process_runtime.startup.as_mut() {
                    startup
                        .replace_environment(self.memory.as_mut().unwrap(), &environment)
                        .map_err(guest)?;
                }
                self.process_runtime.environment = environment;
                1
            }
            ExpandEnvironmentStringsA | ExpandEnvironmentStringsW => {
                let source =
                    buffers::string(self.memory()?, args[0], wide, code_page).map_err(guest)?;
                let expanded = self.process_runtime.environment.expand(&source);
                let length = buffers::encoded(&expanded, wide, code_page).len()
                    / if wide { 2 } else { 1 }
                    + 1;
                buffers::counted(
                    self.memory.as_mut().unwrap(),
                    args[1],
                    args[2],
                    &expanded,
                    wide,
                    code_page,
                )
                .map_err(guest)?;
                length as u32
            }
            GetEnvironmentStringsA | GetEnvironmentStringsW => {
                let code_page = self.windows_oem_code_page();
                let runtime = &mut self.process_runtime;
                let startup = runtime
                    .startup
                    .as_mut()
                    .ok_or(guest(ProcessError32::InvalidParameter))?;
                startup
                    .environment_snapshot(
                        self.memory.as_mut().unwrap(),
                        &runtime.environment,
                        wide,
                        code_page,
                    )
                    .map_err(guest)?
            }
            FreeEnvironmentStringsA | FreeEnvironmentStringsW => {
                self.process_runtime
                    .startup
                    .as_mut()
                    .ok_or(guest(ProcessError32::InvalidParameter))?
                    .free_environment_snapshot(args[0], wide)
                    .map_err(guest)?;
                1
            }
            _ => return Err(HostError::InvalidRequest),
        };
        Ok(result.into())
    }
}
