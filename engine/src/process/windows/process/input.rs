use super::ProcessLaunch32;
use crate::process::{EngineInstance, HostError};

const MAX_INPUT: usize = 1024 * 1024;
const HEADER: usize = 56;
pub(super) struct LaunchInput32 {
    bytes: Vec<u8>,
    received: usize,
}
impl EngineInstance {
    fn check_launch_input(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.image_started
            || self.process_runtime.startup.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }
    pub fn begin_process_launch_input(&mut self, total: u32) -> Result<(), HostError> {
        self.check_launch_input()?;
        if self.process_runtime.launch_input.is_some()
            || !(HEADER..=MAX_INPUT).contains(&(total as usize))
        {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total as usize)
            .map_err(|_| HostError::Infrastructure)?;
        bytes.resize(total as usize, 0);
        self.process_runtime.launch_input = Some(LaunchInput32 { bytes, received: 0 });
        Ok(())
    }
    pub fn append_process_launch_input(
        &mut self,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.check_launch_input()?;
        let input = self
            .process_runtime
            .launch_input
            .as_mut()
            .ok_or(HostError::InvalidRequest)?;
        let start = offset as usize;
        let end = start
            .checked_add(bytes.len())
            .filter(|end| *end <= input.bytes.len())
            .ok_or(HostError::InvalidRequest)?;
        if bytes.is_empty() || start != input.received {
            return Err(HostError::InvalidRequest);
        }
        input.bytes[start..end].copy_from_slice(bytes);
        input.received = end;
        Ok(())
    }
    pub fn abort_process_launch_input(&mut self) -> Result<(), HostError> {
        self.check_launch_input()?;
        self.process_runtime
            .launch_input
            .take()
            .ok_or(HostError::InvalidRequest)?;
        Ok(())
    }
    pub fn commit_process_launch_input(&mut self) -> Result<(), HostError> {
        self.check_launch_input()?;
        let input = self
            .process_runtime
            .launch_input
            .as_ref()
            .filter(|input| input.received == input.bytes.len())
            .ok_or(HostError::InvalidRequest)?;
        let launch = decode(&input.bytes)?;
        self.configure_process_launch(launch)
    }
    pub(in crate::process) fn launch_input_pending(&self) -> bool {
        self.process_runtime.launch_input.is_some()
    }
}

// R3PL v1: a closed 56-byte header, three length-delimited UTF-8 strings, then environment pairs.
fn decode(bytes: &[u8]) -> Result<ProcessLaunch32, HostError> {
    let invalid = || HostError::InvalidRequest;
    if bytes.len() < HEADER || bytes.len() > MAX_INPUT || &bytes[..4] != b"R3PL" {
        return Err(invalid());
    }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if word(4) != 1
        || word(8) as usize != bytes.len()
        || bytes[48..56].iter().any(|byte| *byte != 0)
    {
        return Err(invalid());
    }
    let mut at = HEADER;
    let image_path = text(bytes, &mut at, word(32))?;
    let command_line = text(bytes, &mut at, word(36))?;
    let current_directory = text(bytes, &mut at, word(40))?;
    let count = word(44) as usize;
    if count > (bytes.len() - at) / 8 {
        return Err(invalid());
    }
    let mut environment = Vec::new();
    environment
        .try_reserve_exact(count)
        .map_err(|_| HostError::Infrastructure)?;
    for _ in 0..count {
        let end = at
            .checked_add(8)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(invalid)?;
        let lengths = [
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
            u32::from_le_bytes(bytes[at + 4..end].try_into().unwrap()),
        ];
        at = end;
        let name = text(bytes, &mut at, lengths[0])?;
        let value = text(bytes, &mut at, lengths[1])?;
        environment.push((name, value));
    }
    if at != bytes.len() {
        return Err(invalid());
    }
    let launch = ProcessLaunch32 {
        process_id: word(12),
        wall_clock_filetime: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        startup_bytes: word(24) as usize,
        environment_units: word(28) as usize,
        image_path,
        command_line,
        current_directory,
        environment,
    };
    launch.validate().map_err(HostError::WindowsProcess)?;
    Ok(launch)
}
fn text(bytes: &[u8], at: &mut usize, length: u32) -> Result<String, HostError> {
    let end = at
        .checked_add(length as usize)
        .filter(|end| *end <= bytes.len())
        .ok_or(HostError::InvalidRequest)?;
    let value = std::str::from_utf8(&bytes[*at..end])
        .map_err(|_| HostError::InvalidRequest)?
        .to_owned();
    *at = end;
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> Vec<u8> {
        let strings = ["C:\\game\\fixture.exe", "fixture.exe --mode=guest", "C:\\"];
        let mut bytes = vec![0; HEADER];
        bytes[..4].copy_from_slice(b"R3PL");
        for (at, value) in [(4, 1), (12, 7), (24, 65536), (28, 32768)] {
            bytes[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
        }
        for (index, value) in strings.into_iter().enumerate() {
            bytes[32 + index * 4..36 + index * 4]
                .copy_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
        let length = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        bytes
    }
    #[test]
    fn staged_launch_rejects_truncation_out_of_order_and_replay_without_replacing_profile() {
        let mut engine = EngineInstance::new(8, 7).unwrap();
        let bytes = packet();
        engine
            .begin_process_launch_input(bytes.len() as u32)
            .unwrap();
        assert!(engine.append_process_launch_input(1, &bytes).is_err());
        assert!(engine.commit_process_launch_input().is_err());
        engine.append_process_launch_input(0, &bytes[..20]).unwrap();
        engine
            .append_process_launch_input(20, &bytes[20..])
            .unwrap();
        engine.commit_process_launch_input().unwrap();
        assert_eq!(engine.process_runtime.launch.process_id, 7);
        assert_eq!(
            engine.process_runtime.launch.command_line,
            "fixture.exe --mode=guest"
        );
        assert!(engine.commit_process_launch_input().is_err());
    }
    #[test]
    fn closed_packet_rejects_bad_lengths_reserved_fields_and_trailing_data() {
        let bytes = packet();
        assert_eq!(decode(&bytes).unwrap().process_id, 7);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut bad = bytes.clone();
        bad[48] = 1;
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bad).is_err());
        let mut bad = bytes;
        bad.push(0);
        assert!(decode(&bad).is_err());
    }
}
