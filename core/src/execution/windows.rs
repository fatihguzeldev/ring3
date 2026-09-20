use super::{
    Cpu32, GuestMemory, LoadError, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason,
    load_pe32_with_imports,
};
use crate::PeImportSymbol;

const API_BASE: u32 = 0x7000_0000;
const STACK_BASE: u32 = 0x1000_0000;
const STACK_SIZE: u32 = 64 * 1024;

/// a single guest thread with a minimal win32 import boundary, not a full process.
pub struct Process32 {
    pub memory: GuestMemory,
    pub cpu: Cpu32,
    last_error: u32,
    exit_code: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessStop {
    Exited(u32),
    Stopped(StopReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessResult {
    pub reason: ProcessStop,
    pub instructions: u64,
    pub api_calls: u64,
}

#[derive(Clone, Copy)]
enum Api {
    SetLastError,
    GetLastError,
    ExitProcess,
}

impl Api {
    fn at(address: u32) -> Option<Self> {
        match address.checked_sub(API_BASE)? {
            0 => Some(Self::SetLastError),
            4 => Some(Self::GetLastError),
            8 => Some(Self::ExitProcess),
            _ => None,
        }
    }

    fn resolve(module: &str, symbol: PeImportSymbol<'_>) -> Option<u32> {
        if !module.eq_ignore_ascii_case("kernel32.dll") {
            return None;
        }
        let PeImportSymbol::ByName { name, .. } = symbol else {
            return None;
        };
        let offset = match name {
            "SetLastError" => 0,
            "GetLastError" => 4,
            "ExitProcess" => 8,
            _ => return None,
        };
        Some(API_BASE + offset)
    }
}

impl Process32 {
    /// maps the image, a 64 kib stack and a reserved api page within the page cap.
    ///
    /// # errors
    /// rejects unsupported images/imports, allocation limits and reserved-range
    /// collisions. no windows dll, tls, peb/teb or crt initialization is performed.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn load(bytes: &[u8], page_limit: u32) -> Result<Self, LoadError> {
        let mut image = load_pe32_with_imports(bytes, page_limit, Api::resolve)?;
        image.memory.map_zeroed(
            u64::from(STACK_BASE),
            u64::from(STACK_SIZE),
            Permissions::READ_WRITE,
        )?;
        image
            .memory
            .map_zeroed(u64::from(API_BASE), PAGE_SIZE, Permissions::NONE)?;
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Esp, STACK_BASE + STACK_SIZE);
        Ok(Self {
            memory: image.memory,
            cpu,
            last_error: 0,
            exit_code: None,
        })
    }

    #[must_use]
    pub fn last_error(&self) -> u32 {
        self.last_error
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    /// each guest instruction and each completed api call costs one budget unit.
    /// faults consume no unit for the faulting operation. exit is terminal and
    /// later calls return the same code without executing more guest work.
    pub fn run(&mut self, budget: u64) -> ProcessResult {
        let mut result = ProcessResult {
            reason: ProcessStop::Stopped(StopReason::InstructionLimit),
            instructions: 0,
            api_calls: 0,
        };
        if let Some(code) = self.exit_code {
            result.reason = ProcessStop::Exited(code);
            return result;
        }
        let mut remaining = budget;
        while remaining != 0 {
            let step = self.cpu.run_until(&mut self.memory, remaining, |address| {
                Api::at(address).is_some()
            });
            result.instructions += step.instructions;
            remaining -= step.instructions;
            if step.reason != StopReason::Intercepted {
                result.reason = ProcessStop::Stopped(step.reason);
                return result;
            }
            let Some(api) = Api::at(self.cpu.eip) else {
                unreachable!()
            };
            if let Err(error) = self.dispatch(api) {
                result.reason = ProcessStop::Stopped(StopReason::MemoryFault(error));
                return result;
            }
            result.api_calls += 1;
            remaining -= 1;
            if let Some(code) = self.exit_code {
                result.reason = ProcessStop::Exited(code);
                return result;
            }
        }
        result
    }

    fn dispatch(&mut self, api: Api) -> Result<(), MemoryError> {
        let stack = self.cpu.register(Register32::Esp);
        let length = if matches!(api, Api::GetLastError) {
            4
        } else {
            8
        };
        stack
            .checked_add(length - 1)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut frame = [0; 8];
        self.memory
            .read(u64::from(stack), &mut frame[..length as usize])?;
        let return_address = u32::from_le_bytes(frame[..4].try_into().unwrap());
        let argument = u32::from_le_bytes(frame[4..].try_into().unwrap());
        match api {
            Api::SetLastError => self.last_error = argument,
            Api::GetLastError => self.cpu.set_register(Register32::Eax, self.last_error),
            Api::ExitProcess => {
                self.exit_code = Some(argument);
                return Ok(());
            }
        }
        self.cpu
            .set_register(Register32::Esp, stack.wrapping_add(length));
        self.cpu.eip = return_address;
        Ok(())
    }
}
