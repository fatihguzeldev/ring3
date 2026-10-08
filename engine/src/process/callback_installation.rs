#![forbid(unsafe_code)]

use super::{
    CallError, EngineInstance, HostError, ResidentInstallation, callback::SuspendedRecord,
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, X87_OFFSET},
        x86::{
            EXIT_SIZE, EXIT_VERSION_3, EXIT_VERSION_5, STATE_SIZE, X87_SIZE, decode_exit, decode_state, decode_x87,
        },
    },
    cpu::{ExitReason, dbt::UnitId},
};

impl EngineInstance {
    pub fn compile_resident_callback_unit(
        &mut self,
        key: u64,
        home_id: u64,
        callback_token: u32,
        count: u32,
        gate_count: u32,
    ) -> Result<UnitId, HostError> {
        let pc = self.resident_callback_installation_pc(key, home_id, callback_token)?;
        self.compile_resident_descriptors(count, gate_count, Some(pc))
    }

    pub fn acknowledge_resident_callback_installation(
        &mut self,
        key: u64,
        home_id: u64,
        callback_token: u32,
        unit_id: u64,
        slot: u32,
    ) -> Result<ResidentInstallation, HostError> {
        let pc = self.resident_callback_installation_pc(key, home_id, callback_token)?;
        let target = self.guard_resident_unit(key, unit_id)?;
        let record = self
            .callback
            .as_ref()
            .unwrap()
            .authorized_resident_record()
            .unwrap();
        if unit_id == record.outer_unit_id || !target.contains_instruction(pc) {
            return Err(HostError::InvalidRequest);
        }
        self.acknowledge_resident_slot(unit_id, slot)
    }

    fn resident_callback_installation_pc(
        &self,
        key: u64,
        home_id: u64,
        callback_token: u32,
    ) -> Result<u32, HostError> {
        self.guard_resident_unit(key, home_id)?;
        let callback = self
            .callback
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let SuspendedRecord::Resident {
            record,
            authorized,
            active_unit_id,
        } = callback.record
        else {
            return Err(HostError::Call(CallError::InvalidToken));
        };
        if callback_token == 0
            || callback_token != record.token
            || home_id != record.callback_unit_id
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.guard_resident_unit(key, record.outer_unit_id)?;
        if !authorized || self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.guard_resident_unit(key, active_unit_id)?;
        decode_x87(&self.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let exit_bytes = &self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE];
        let exit = decode_exit(exit_bytes).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if !matches!(
            u16::from_le_bytes([exit_bytes[4], exit_bytes[5]]),
            EXIT_VERSION_3 | EXIT_VERSION_5
        ) || exit.reason != ExitReason::NeedCode
        {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        Ok(state.eip)
    }
}
