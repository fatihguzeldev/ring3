use super::context::Bstr32;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, windows::strings::Input32},
    windows::automation::{AutomationError32 as E, Oleaut32Api32 as Api, bstr_bytes},
};
impl EngineInstance {
    pub(super) fn automation_string(&mut self, api: Api, args: &[u32]) -> Result<u32, HostError> {
        use Api::*;
        if matches!(api, SysStringLen | SysStringByteLen) {
            if args[0] == 0 {
                return Ok(0);
            }
            let at = args[0]
                .checked_sub(4)
                .ok_or(HostError::Automation(E::InvalidParameter))?;
            let mut header = [0; 4];
            self.memory()?
                .read(GuestAddress(at), &mut header)
                .map_err(HostError::Memory)?;
            let length = u32::from_le_bytes(header);
            return Ok(if api == SysStringLen {
                length / 2
            } else {
                length
            });
        }
        if api == SysFreeString {
            self.free_automation_string(args[0])?;
            return Ok(0);
        }
        let realloc = matches!(api, SysReAllocString | SysReAllocStringLen);
        let mut replace = None;
        let source = if realloc {
            if args[0] == 0 {
                return Err(HostError::Automation(E::InvalidParameter));
            }
            self.memory()?
                .check_span(GuestAddress(args[0]), 4, Access::Write)
                .map_err(HostError::Memory)?;
            let mut old = [0; 4];
            self.memory()?
                .read(GuestAddress(args[0]), &mut old)
                .map_err(HostError::Memory)?;
            let old = u32::from_le_bytes(old);
            if old != 0 {
                let index = self
                    .automation
                    .strings
                    .iter()
                    .position(|row| row.pointer == old)
                    .ok_or(HostError::Automation(E::InvalidParameter))?;
                let row = self.automation.strings[index];
                self.check_windows_runtime_bytes(old - 4, row.bytes as u32)?;
                replace = Some(index);
            }
            args[1]
        } else {
            args[0]
        };
        if source == 0 && matches!(api, SysAllocString | SysReAllocString) {
            if realloc {
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[0]), &[0; 4])
                    .map_err(HostError::Memory)?;
                if let Some(index) = replace {
                    let old = self.automation.strings[index].pointer;
                    self.free_automation_string(old)?;
                }
                return Ok(1);
            }
            return Ok(0);
        }
        let payload = if matches!(api, SysAllocString | SysReAllocString) {
            let input = Input32::read(
                self.memory()?,
                source,
                true,
                self.windows_ansi_code_page(),
                self.automation.limits.bytes / 2,
            )
            .map_err(|e| match e {
                super::super::strings::WindowsStringError32::Memory(e) => HostError::Memory(e),
                _ => HostError::Automation(E::InvalidParameter),
            })?;
            input
                .units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            let length = if api == SysAllocStringByteLen {
                args[1] as usize
            } else {
                (args[if realloc { 2 } else { 1 }] as usize)
                    .checked_mul(2)
                    .ok_or(HostError::Automation(E::Capacity))?
            };
            if length
                .checked_add(6)
                .is_none_or(|n| n > self.automation.limits.bytes)
            {
                return Err(HostError::Automation(E::Capacity));
            }
            if source != 0 {
                self.memory()?
                    .check_span(GuestAddress(source), length, Access::Read)
                    .map_err(HostError::Memory)?;
            }
            let mut payload = Vec::new();
            payload
                .try_reserve_exact(length)
                .map_err(|_| HostError::Automation(E::Allocation))?;
            payload.resize(length, 0);
            if source != 0 {
                self.memory()?
                    .read(GuestAddress(source), &mut payload)
                    .map_err(HostError::Memory)?;
            }
            payload
        };
        let bytes =
            bstr_bytes(&payload, self.automation.limits.bytes).map_err(HostError::Automation)?;
        self.automation
            .prepare_string(bytes.len(), replace)
            .map_err(HostError::Automation)?;
        let prepared = self.prepare_windows_local_initialized(bytes.len() as u32)?;
        let base = prepared.address();
        let pointer = base
            .checked_add(4)
            .ok_or(HostError::Automation(E::Capacity))?;
        let encoded = pointer.to_le_bytes();
        let output = [(GuestAddress(args[0]), encoded.as_slice())];
        let outputs = if realloc { &output[..] } else { &[] };
        self.commit_windows_local_initialized(prepared, &bytes, outputs)?;
        let row = Bstr32 {
            pointer,
            bytes: bytes.len(),
        };
        if let Some(index) = replace {
            let old = self.automation.strings[index];
            self.free_windows_runtime_bytes(old.pointer - 4);
            self.automation.bytes = self.automation.bytes - old.bytes + row.bytes;
            self.automation.strings[index] = row;
        } else {
            self.automation.bytes += row.bytes;
            self.automation.strings.push(row);
        }
        Ok(if realloc { 1 } else { pointer })
    }
    pub(super) fn free_automation_string(&mut self, pointer: u32) -> Result<(), HostError> {
        if pointer == 0 {
            return Ok(());
        }
        let index = self
            .automation
            .strings
            .iter()
            .position(|row| row.pointer == pointer)
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        let row = self.automation.strings[index];
        self.check_windows_runtime_bytes(pointer - 4, row.bytes as u32)?;
        self.release_windows_runtime_bytes(pointer - 4)?;
        self.automation.strings.remove(index);
        self.automation.bytes -= row.bytes;
        Ok(())
    }
    pub(in crate::process::windows::automation) fn automation_bstr_size(
        &self,
        pointer: u32,
    ) -> Result<usize, HostError> {
        if pointer == 0 {
            return Ok(0);
        }
        let at = pointer
            .checked_sub(4)
            .ok_or(HostError::Automation(E::InvalidParameter))?;
        let mut length = [0; 4];
        self.memory()?
            .read(GuestAddress(at), &mut length)
            .map_err(HostError::Memory)?;
        let length = u32::from_le_bytes(length) as usize;
        if length > self.automation.limits.bytes.saturating_sub(6) {
            return Err(HostError::Automation(E::Capacity));
        }
        Ok(length)
    }

    pub(in crate::process::windows::automation) fn read_automation_bstr(
        &self,
        pointer: u32,
    ) -> Result<Vec<u8>, HostError> {
        if pointer == 0 {
            return Ok(Vec::new());
        }
        let length = self.automation_bstr_size(pointer)?;
        self.memory()?
            .check_span(GuestAddress(pointer), length, Access::Read)
            .map_err(HostError::Memory)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.resize(length, 0);
        self.memory()?
            .read(GuestAddress(pointer), &mut bytes)
            .map_err(HostError::Memory)?;
        Ok(bytes)
    }
}
