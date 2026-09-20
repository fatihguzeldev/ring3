use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions, ProcessStop};
use crate::execution::loader::modules::Initializer;

const BASE: u32 = 0x7001_4000;

#[derive(Default)]
pub(super) struct Startup {
    failures: Vec<(u32, String)>,
    failed: Option<String>,
    completion: Option<u32>,
}

impl Startup {
    pub(super) fn map(
        memory: &mut GuestMemory,
        initializers: Vec<Initializer>,
        entry: u32,
    ) -> Result<(Self, u32), MemoryError> {
        let mut startup = Self::default();
        if initializers.is_empty() {
            return Ok((startup, entry));
        }
        let mut code = Vec::new();
        for initializer in initializers {
            // static process attach uses a non-null reserved value.
            code.extend_from_slice(&[0x6a, 1, 0x6a, 1, 0x68]);
            code.extend_from_slice(&initializer.base.to_le_bytes());
            code.push(0xb8);
            code.extend_from_slice(&initializer.entry.to_le_bytes());
            code.extend_from_slice(&[0xff, 0xd0, 0x83, 0xf8, 0, 0x75, 1]);
            startup.failures.push((
                BASE + u32::try_from(code.len()).expect("bounded module count"),
                initializer.name,
            ));
            code.push(0xcc);
        }
        startup.completion = Some(BASE + u32::try_from(code.len()).expect("bounded module count"));
        code.push(0xe9);
        let next = BASE + u32::try_from(code.len()).expect("bounded module count") + 4;
        code.extend_from_slice(&entry.wrapping_sub(next).to_le_bytes());
        memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
        memory.write(u64::from(BASE), &code)?;
        memory.protect(u64::from(BASE), PAGE_SIZE, Permissions::READ_EXECUTE)?;
        Ok((startup, BASE))
    }

    pub(super) fn contains(&self, address: u32) -> bool {
        self.completion == Some(address) || self.failures.iter().any(|(trap, _)| *trap == address)
    }

    pub(super) fn complete_at(&mut self, address: u32) -> bool {
        if self.completion != Some(address) {
            return false;
        }
        self.completion = None;
        true
    }

    pub(super) fn is_complete(&self) -> bool {
        self.completion.is_none()
    }

    pub(super) fn stop(&mut self, address: u32) -> Option<ProcessStop> {
        let (_, name) = self.failures.iter().find(|(trap, _)| *trap == address)?;
        self.failed = Some(name.clone());
        self.failed()
    }

    pub(super) fn failed(&self) -> Option<ProcessStop> {
        self.failed
            .as_ref()
            .map(|module| ProcessStop::DllInitializationFailed {
                module: module.clone(),
            })
    }
}
