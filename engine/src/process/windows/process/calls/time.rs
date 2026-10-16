use super::*;

impl EngineInstance {
    pub(super) fn execute_process_time_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let result = match api {
            QueryPerformanceCounter | QueryPerformanceFrequency => {
                let value = if api == QueryPerformanceCounter {
                    self.cpu_clock.timestamp()?
                } else {
                    self.cpu_clock.frequency()?
                };
                buffers::write(self.memory.as_mut().unwrap(), args[0], &value.to_le_bytes())
                    .map_err(guest)?;
                1
            }
            GetTickCount | GetTickCount64 => {
                let (epoch, now) = self.cpu_clock.monotonic_bounds()?;
                return Ok(ProcessReply32 {
                    value: (now - epoch) / 1_000_000,
                    error: None,
                });
            }
            GetSystemTimeAsFileTime => {
                let value = self.windows_filetime()?;
                buffers::write(self.memory.as_mut().unwrap(), args[0], &value.to_le_bytes())
                    .map_err(guest)?;
                0
            }
            _ => return Err(HostError::InvalidRequest),
        };
        Ok(result.into())
    }
}
